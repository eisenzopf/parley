// Complete local story. The harness simulates people/providers; the independently
// running assistant controls all communications through the real UCTP client.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { chmod, mkdir, readFile, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { chromium, expect } from '@playwright/test';
import { UctpClient, UctpError } from '../clients/uctp-js/client.mjs';
import { collectEvidence } from '../clients/uctp-js/evidence.mjs';
import { saveEvidence } from '../scripts/export-conference-evidence.mjs';
import { ConferenceWorker, FileState, runWorkerLoop } from '../examples/conference-assistant/worker.mjs';
import { VapiPlanner } from '../examples/conference-assistant/vapi.mjs';

const fixture = JSON.parse(process.env.CONFERENCE_SCENARIO_FIXTURE);
const scenario = fixture.scenario;
assert.equal(scenario.sandbox, true);
const voiceOnly = process.env.CONFERENCE_DEMO_MODE === 'voice-only';
const livePlanner = process.env.CONFERENCE_LIVE_PLANNER === '1';
const liveVoice = fixture.live_voice === true;
assert.ok(!liveVoice || livePlanner, 'Live voice scenario requires real planning');
if (livePlanner && !process.env.VAPI_PRIVATE_KEY) throw new Error('VAPI_PRIVATE_KEY required for live-planner rehearsal');
const plannerTimeout = livePlanner ? 120000 : 25000;
const screenshotPrefix = (voiceOnly ? 'voice-only-' : '') + (liveVoice ? 'conference-live-vapi' : livePlanner ? 'conference-live-planner' : 'conference-scenario');
const recording = process.env.CONFERENCE_RECORD === '1';
const recordingDir = `test-results/recordings/${screenshotPrefix}-${fixture.cid}`;
const pauseForRecording = async milliseconds => { if (recording) await new Promise(resolve => setTimeout(resolve, milliseconds)); };
const actions = []; let decisions = 0;
const plannerApi = createServer(async (request, response) => {
  try {
    assert.equal(request.url, '/chat'); assert.equal(request.headers.authorization, 'Bearer fixture-planner');
    let raw = ''; for await (const chunk of request) raw += chunk;
    const body = JSON.parse(raw); assert.deepEqual(body.assistant.model.tools, []);
    const context = JSON.parse(body.input); assert.equal(context.cid, fixture.cid);
    const role = name => context.participants.find(p => p.role === name).participant_id;
    const events = context.events;
    const calls = events.filter(e => e.event_type === 'session.invited');
    const booker = calls.find(e => e.payload.participant_id === role('booker'));
    const organizer = calls.find(e => e.payload.participant_id === role('organizer'));
    const ended = call => call && events.some(e => e.event_type === 'session.ended' && e.payload.sid === call.payload.sid);
    const transcript = who => events.find(e => e.event_type === 'session.transcript' && e.payload.speaker === role(who));
    const organizerText = events.find(e => e.event_type === 'message.accepted' && e.payload.delivery === 'sms' && e.payload.from === role('assistant'));
    const reply = events.find(e => e.event_type === 'message.received' && e.payload.from === role('organizer'));
    const browserInvitation = events.find(e => e.event_type === 'message.accepted' && e.payload.from === role('assistant')
      && e.payload.content_type === 'application/json' && JSON.parse(e.payload.body).type === 'travel.browser_invitation');
    let next = [];
    if (!booker) next = [{ type: 'call_participant', to: role('booker'), purpose: 'Find a sandbox alternative for Jonathan and Alex after their flight was canceled.' }];
    else if (!ended(booker) && transcript('booker')) next = [{ type: 'end_voice', sid: booker.payload.sid }];
    else if (voiceOnly && ended(booker) && !organizer) next = [{ type: 'call_participant', to: role('organizer'), purpose: `Confirm sandbox pickup from the booker itinerary: ${transcript('booker').payload.text}. Keep the call open for Jonathan to join.` }];
    else if (!voiceOnly && ended(booker) && !organizerText) next = [{ type: 'message', to: [role('organizer')], delivery: 'sms', body: `Sandbox itinerary: ${transcript('booker').payload.text} Can you confirm pickup?` }];
    else if (reply && !organizer) next = [{ type: 'call_participant', to: role('organizer'), purpose: `Confirm sandbox pickup. Booker: ${transcript('booker').payload.text} Organizer reply: ${reply.payload.body}` }];
    else if (organizer && !ended(organizer) && transcript('organizer') && !browserInvitation)
      next = [{ type: 'request_browser_join', sid: organizer.payload.sid }];
    else if (ended(organizer) && !context.proposal) {
      assert.ok(events.some(e => e.event_type === 'browser.speaking' && e.payload.sid === organizer.payload.sid));
      next = [{ type: 'propose_arrangements', summary: scenario.proposal }];
    } else if (voiceOnly && context.approval && context.voice_rehearsal_completed_for !== context.approval.proposal_id) next = [{ type: 'complete_voice_rehearsal', proposal_id: context.approval.proposal_id }];
    else if (!voiceOnly && context.approval && context.final_updates_accepted_for !== context.approval.proposal_id) next = [{ type: 'final_updates', proposal_id: context.approval.proposal_id,
      updates: context.participants.filter(p => p.role !== 'assistant').map(p => ({ to: p.participant_id, body: `${p.name}: RD742 departs at 16:00, arrives terminal C at 17:00; organizer pickup confirmed.` })) }];
    decisions++;
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ id: `fixture-chat-${decisions}`, output: [{ role: 'assistant', content: JSON.stringify({ actions: next }) }] }));
  } catch (error) { response.writeHead(500); response.end(error.message); }
});
plannerApi.listen(0, '127.0.0.1'); await once(plannerApi, 'listening');
const assistant = new UctpClient(fixture.url, fixture.assistant_token, { timeoutMs: 20000,
  trace: (direction, frame) => { if (direction === 'send' && ['message.send', 'session.invite', 'session.end'].includes(frame.type)) actions.push(frame); },
});
const ownerCheck = new UctpClient(fixture.url, fixture.owner_token, { timeoutMs: 20000 });
const browser = await chromium.launch({ channel: (process.env.CI || process.env.PARLEY_BROWSER_CHANNEL === 'chromium') ? 'chromium' : 'chrome',
  args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
});
let stop = false, workerError, worker, run, context, video, scenarioPassed = false, evidencePath, failureEvidenceSaved = false;
try {
  await assistant.connect();
  await ownerCheck.connect();
  const roleIds = Object.fromEntries((await assistant.snapshot(fixture.cid)).participants.map(p => [p.role, p.participant_id]));
  let lostSnapshotReply = false;
  if (!livePlanner) {
    const request = assistant.request.bind(assistant);
    assistant.request = async frame => {
      const response = await request(frame);
      if (!lostSnapshotReply && frame.type === 'conversation.subscribe'
        && response.payload.events?.some(e => e.event_type === 'session.transcript' && e.payload.speaker === roleIds.booker)) {
        lostSnapshotReply = true;
        throw new UctpError('UCTP request timed out; outcome may be unknown', { request: frame });
      }
      return response;
    };
  }
  const spokenBy = (events, role) => events.filter(e => e.event_type === 'session.transcript' && e.payload.speaker === roleIds[role])
    .map(e => e.payload.text).join(' ');
  const providerPlanner = livePlanner
    ? new VapiPlanner({ apiKey: process.env.VAPI_PRIVATE_KEY, model: process.env.VAPI_CHAT_MODEL || undefined })
    : new VapiPlanner({ apiKey: 'fixture-planner', endpoint: `http://127.0.0.1:${plannerApi.address().port}/chat` });
  worker = new ConferenceWorker({ client: assistant, cid: fixture.cid,
    planner: { decide: async context => {
      if (livePlanner && ++decisions > 20) throw new Error('Live planner exceeded the rehearsal decision limit');
      const result = await providerPlanner.decide(context);
      if (livePlanner) console.log(JSON.stringify({ event: 'live.planner.decision', decision: decisions,
        actions: result.actions.map(a => ({ type: a.type, delivery: a.delivery,
          sid: a.sid,
          target_roles: [a.to].flat().filter(Boolean).map(id => context.participants.find(p => p.participant_id === id)?.role || 'unknown') })),
        waiting_for: result.waiting_for, provider_chat_received: !!result.provider_chat_id }));
      return result;
    } },
    storage: new FileState(fixture.worker_state), mode: voiceOnly ? 'voice-only' : 'full',
    log: entry => console.log(JSON.stringify(entry)),
  });
  run = runWorkerLoop({ client: assistant, worker, stopped: () => stop,
    pause: () => new Promise(resolve => setTimeout(resolve, 100)), log: entry => console.log(JSON.stringify(entry)),
  }).catch(error => { workerError = error; console.error(JSON.stringify({ event: 'scenario.worker.failed', message: error.message, code: error.code })); });
  if (recording) await mkdir(recordingDir, { recursive: true, mode: 0o700 });
  context = await browser.newContext({ viewport: { width: 1600, height: 1100 },
    ...(recording ? { recordVideo: { dir: recordingDir, size: { width: 1600, height: 1100 } } } : {}) });
  const page = await context.newPage(); video = page.video();
  await page.addInitScript(() => {
    const NativeSocket = window.WebSocket;
    window.WebSocket = class extends NativeSocket { constructor(...args) { super(...args); window.scenarioControlSocket = this; } };
    const NativeAudio = window.AudioContext;
    window.scenarioRingTones = 0;
    window.AudioContext = class extends NativeAudio {
      createOscillator() {
        const oscillator = super.createOscillator(), start = oscillator.start.bind(oscillator);
        oscillator.start = (...args) => {
          if ([440, 480].includes(oscillator.frequency.value)) window.scenarioRingTones++;
          return start(...args);
        };
        return oscillator;
      }
    };
  });
  page.on('pageerror', error => { workerError ||= error; });
  await page.goto(`${fixture.http}/conference/${voiceOnly ? '?mode=voice-only' : ''}`);
  await page.evaluate(({ live, voice, recording }) => {
    const label = document.createElement('p'); label.textContent = voice
      ? 'LIVE VAPI PLANNING + VOICE · Synthetic local SIP participants. SMS is a fixture.' : live
      ? 'LIVE VAPI PLANNING · Simulated people, voice provider and SMS. Real local SIP/browser audio.'
      : 'LOCAL REHEARSAL · Simulated people/providers. Real SIP and browser audio.';
    if (recording) label.textContent = `RECORDED REHEARSAL · SILENT VIDEO · ${label.textContent}`;
    label.style.cssText = 'margin:4px 0 0;color:#f6c986;font-size:10px'; document.querySelector('header > div').append(label);
  }, { live: livePlanner, voice: liveVoice, recording });
  await page.locator('#url').fill(fixture.url); await page.locator('#cid').fill(fixture.cid); await page.locator('#token').fill(fixture.owner_token);
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.locator('#status')).toContainText(voiceOnly ? 'SMS deferred' : 'SMS fixture');
  await pauseForRecording(2000);
  await page.locator('#task').fill(scenario.task);
  await page.getByRole('button', { name: 'Start coordinating' }).click();
  await expect(page.locator('#join')).toBeDisabled();
  const poll = async (fn, message) => {
    const deadline = Date.now() + plannerTimeout;
    while (Date.now() < deadline) {
      if (workerError) throw workerError;
      if (worker.state?.events.some(e => e.event_type === 'session.assistant_failed')) throw new Error('The voice assistant attachment failed; the conference task did not complete');
      if (await fn()) return;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    await mkdir('test-results', { recursive: true });
    await saveEvidence(`test-results/${screenshotPrefix}-${fixture.cid}-failure.json`, { failure: message,
      ...(await collectEvidence(assistant, fixture.cid, { mode: liveVoice ? 'live-vapi-fixture-sms' : livePlanner ? 'live-planner-fixture-voice-sms' : 'local-fixture' })) });
    failureEvidenceSaved = true;
    throw new Error(message);
  };
  if (!voiceOnly) {
  // Observe independently while the worker recovers its deliberately dropped
  // snapshot response. The observer must not use the worker's reconnecting socket.
  await poll(async () => (await ownerCheck.history(fixture.cid)).some(m => m.medium === 'sms'), 'Assistant did not finish the booker call and text the organizer');
  const deliveryEvents = () => ownerCheck.snapshot(fixture.cid).then(s => s.events);
  await poll(async () => (await deliveryEvents()).some(e => e.event_type === 'message.delivery' && e.payload.state === 'sent'), 'Organizer SMS did not reach provider acceptance');
  }
  const events = () => ownerCheck.snapshot(fixture.cid).then(s => s.events);
  await poll(async () => (await events()).filter(e => e.event_type === 'session.invited').length === (voiceOnly ? 2 : 1), 'Expected sequential voice invitations');
  let snapshot = await events();
  const invitedBooker = snapshot.find(e => e.event_type === 'session.invited');
  assert.ok(snapshot.some(e => e.event_type === 'session.ended' && e.payload.sid === invitedBooker.payload.sid));
  if (liveVoice) {
    const text = spokenBy(snapshot, 'booker');
    assert.match(text, /r\s*\.?\s*d\s*\.?\s*(742|7\s+4\s+2|seven\s+four\s+two)/i, 'Booker speech must supply the flight');
    if (voiceOnly) {
      assert.match(text, /(?:good\s*bye|\bbye\b)/i, 'The booker fixture must finish with an attributed goodbye');
      const bookerCall = snapshot.find(e => e.event_type === 'session.invited' && e.payload.participant_id === roleIds.booker);
      assert.equal(actions.filter(a => a.type === 'session.end' && a.sid === bookerCall.payload.sid).length, 1,
        'David must hang up through UCTP after the booker goodbye rather than relying on a provider timeout');
    }
    assert.match(text, /terminal\s+c/i);
    assert.match(text, voiceOnly ? /11|eleven/i : /16|sixteen|4(?::00)?\s*p\.?m/i);
    assert.match(text, voiceOnly ? /16|sixteen|4(?::00)?\s*p\.?m/i : /17|seventeen|5(?::00)?\s*p\.?m/i);
  } else assert.ok(snapshot.some(e => e.event_type === 'session.transcript' && e.payload.text === scenario.booker_transcript));
  await pauseForRecording(2000);
  // The harness acts as the local SMS-provider fixture, never as an assistant tool.
  if (!voiceOnly) {
  const incoming = await fetch(`${fixture.http}/v1/sms/inbound`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ data: {
    id: 'fixture-organizer-event', event_type: 'message.received', occurred_at: new Date().toISOString(), payload: {
      id: 'fixture-organizer-reply', direction: 'inbound', from: { phone_number: '+14155550104' }, to: [{ phone_number: '+14155550000' }], text: scenario.organizer_reply,
    },
  } }) });
  assert.equal(incoming.status, 200);
  }
  await poll(async () => {
    snapshot = await events();
    if (liveVoice) {
      const text = spokenBy(snapshot, 'organizer');
      return /terminal\s+c/i.test(text) && /pick\s?up/i.test(text) && /Jonathan/i.test(text)
        && (voiceOnly ? /16|sixteen|4(?::00)?\s*p\.?m/i : /17|seventeen|5(?::00)?\s*p\.?m/i).test(text) && /bring|join/i.test(text);
    }
    return snapshot.some(e => e.event_type === 'session.transcript' && e.payload.text === scenario.organizer_transcript);
  }, 'Organizer confirmation call did not produce an attributed transcript');
  const invitations = snapshot.filter(e => e.event_type === 'session.invited'); assert.equal(invitations.length, 2);
  const organizerCall = invitations[1].payload;
  await expect(page.locator('#join')).toBeEnabled({ timeout: plannerTimeout });
  await expect(page.locator('#join')).toHaveText('Answer organizer call');
  await expect(page.locator('#browser-invitation')).toContainText('Organizer call ready');
  await expect.poll(() => page.evaluate(() => window.scenarioRingTones)).toBeGreaterThan(0);
  await page.screenshot({ path: `test-results/${screenshotPrefix}-ringing.png`, fullPage: true });
  if (!livePlanner) assert.equal(lostSnapshotReply, true, 'Actual worker loop must recover the simulated lost snapshot response');
  await expect(page.locator('[data-edge=vapi]')).toHaveAttribute('data-state', 'active');
  await expect(page.locator('#network-ids')).toContainText(organizerCall.connid);
  await pauseForRecording(3000);
  const before = await (await fetch(`${fixture.http}/__scenario_fixture`)).json(); assert.ok(before.ai_frames > 10);
  if (liveVoice) {
    await poll(async () => (await (await fetch(`${fixture.http}/__scenario_fixture`)).json()).speech_sent, 'Organizer must finish their spoken confirmation');
    assert.equal((await fetch(`${fixture.http}/__scenario_tone`, { method: 'POST' })).status, 200);
  }
  // Replace only the test microphone with a known tone. The actual stage button
  // still negotiates ICE/DTLS and commits the UCTP handoff.
  await page.evaluate(async () => {
    const NativePeer = window.RTCPeerConnection;
    window.RTCPeerConnection = class extends NativePeer { constructor(...args) { super(...args); window.scenarioPeer = this; } };
    const source = new AudioContext(); await source.resume();
    const oscillator = source.createOscillator(); oscillator.frequency.value = 880;
    const gain = source.createGain(); gain.gain.value = 0.3;
    const destination = source.createMediaStreamDestination(); oscillator.connect(gain).connect(destination); oscillator.start();
    window.scenarioSource = source;
    navigator.mediaDevices.getUserMedia = async () => destination.stream;
  });
  await page.locator('#join').click();
  await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state', 'speaking', { timeout: 20000 });
  await expect(page.locator('[data-edge=vapi]')).toHaveAttribute('data-state', 'retired');
  await expect(page.locator('#retained')).toContainText('Same telephone Connection retained');
  await expect(page.locator('#network-ids')).toContainText(organizerCall.connid);
  const measured = await page.evaluate(async () => {
    const audio = document.getElementById('remote-audio');
    const sink = new AudioContext(); await sink.resume(); const analyzer = sink.createAnalyser(); analyzer.fftSize = 4096;
    sink.createMediaStreamSource(audio.srcObject).connect(analyzer);
    const bins = new Float32Array(analyzer.frequencyBinCount); let audible = 0, peak = 0, inbound = 0, outbound = 0, codec;
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      analyzer.getFloatFrequencyData(bins); let strongest = -Infinity;
      for (let i = 1; i < bins.length; i++) if (bins[i] > strongest) { strongest = bins[i]; peak = i * sink.sampleRate / analyzer.fftSize; }
      if (Math.abs(peak - 660) < 35 && strongest > -50) audible++;
      const stats = await window.scenarioPeer.getStats();
      for (const stat of stats.values()) {
        if (stat.type === 'inbound-rtp' && stat.kind === 'audio') inbound = stat.packetsReceived;
        if (stat.type === 'outbound-rtp' && stat.kind === 'audio') { outbound = stat.packetsSent; codec = stats.get(stat.codecId)?.mimeType; }
      }
      if (audible >= 8 && inbound > 20 && outbound > 20) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    return { audible, peak, inbound, outbound, codec };
  });
  assert.ok(measured.audible >= 8 && measured.inbound > 20 && measured.outbound > 20, JSON.stringify(measured));
  assert.equal(measured.codec?.toLowerCase(), 'audio/opus');
  const after = await (await fetch(`${fixture.http}/__scenario_fixture`)).json(); assert.ok(after.human_frames > 10); assert.equal(after.ended, false);
  const room = await page.evaluate(() => ({
    viewport: innerHeight,
    pageHeight: document.documentElement.scrollHeight,
    endBottom: document.querySelector('#end').getBoundingClientRect().bottom,
    footerBottom: document.querySelector('#contribution-panel').getBoundingClientRect().bottom,
  }));
  assert.ok(room.pageHeight <= room.viewport + 1 && room.footerBottom <= room.viewport && room.endBottom <= room.viewport, JSON.stringify(room));
  await mkdir('test-results', { recursive: true });
  await page.screenshot({ path: `test-results/${screenshotPrefix}-handoff.png`, fullPage: true });
  await pauseForRecording(3000);
  // The actual owner stage control requests a callback inside the same Session.
  const browserJoined = (await events()).find(e => e.event_type === 'browser.speaking');
  const phoneCommand = (client, payload = { kind: 'move_to_phone' }) => client.command('session.update', fixture.cid, payload, { sid: organizerCall.sid, connid: browserJoined.payload.connid });
  await assert.rejects(assistant.request(phoneCommand(assistant)), error => error.code === 403);
  await assert.rejects(ownerCheck.request(phoneCommand(ownerCheck, { kind: 'move_to_phone', phone_number: '+14155559999' })), error => error.code === 400);
  await expect(page.locator('#move-phone')).toBeEnabled();
  const fixtureStats = async () => (await (await fetch(`${fixture.http}/__scenario_fixture`)).json());
  // Earlier live speech can contribute energy near the test frequency. Compare
  // changes during the callback instead of assuming a lifetime counter is zero.
  const beforeCallback = await fixtureStats();
  await page.locator('#move-phone').click();
  await poll(async () => (await events()).some(e => e.event_type === 'phone.answered'), 'First callback did not answer');
  await poll(async () => {
    const journal = await events();
    const failed = journal.find(e => e.event_type === 'phone.failed');
    if (failed) throw new Error(`Callback prompt failed: ${JSON.stringify(failed.payload.details)}`);
    return (await fixtureStats()).phone_pending_audio > 5;
  }, 'Callback must receive an audible confirmation prompt before pressing 1');
  let phoneEvents = (await events()).filter(e => e.event_type.startsWith('phone.'));
  const firstPhone = phoneEvents.find(e => e.event_type === 'phone.prepared');
  const original = (await ownerCheck.request(ownerCheck.command('conversation.inspect', fixture.cid, { request_id: firstPhone.request_id }))).payload.evidence;
  assert.deepEqual(await ownerCheck.request(original.request), original.response, 'Exact replay must return the original acceptance');
  await assert.rejects(ownerCheck.request(phoneCommand(ownerCheck)), error => error.code === 409);
  const pendingBefore = await fixtureStats();
  await fetch(`${fixture.http}/__scenario_phone_digit`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ digit: '9' }) });
  await new Promise(resolve => setTimeout(resolve, 500));
  assert.equal((await events()).filter(e => e.event_type === 'phone.speaking').length, 0, 'Answering or wrong DTMF must not move audio');
  const pendingAfter = await fixtureStats();
  assert.equal(pendingAfter.phone_answered, 1); assert.equal(pendingAfter.phone_frames, beforeCallback.phone_frames, 'Unconfirmed callback audio must not reach the retained telephone');
  assert.ok(pendingAfter.human_frames > pendingBefore.human_frames, 'Browser audio must continue while callback awaits confirmation');
  assert.equal(pendingAfter.ended, false);
  await page.locator('#cancel-phone').click();
  await poll(async () => (await events()).some(e => e.event_type === 'phone.cancelled'), 'First callback was not cancelled');
  await poll(async () => (await fixtureStats()).phone_ended === 1, 'Cancelled callback SIP resource must end');
  await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state', 'speaking');
  await expect(page.locator('#move-phone')).toBeEnabled();
  await page.locator('#move-phone').click();
  await poll(async () => (await events()).filter(e => e.event_type === 'phone.answered').length === 2, 'Second callback did not answer');
  const waitingBefore = await fixtureStats();
  // The live carrier previously cleared a silent, unbridged callback after
  // about 21 seconds. Exercise a longer pre-confirmation window with real
  // RTP, repeated spoken prompts and in-band G.711 silence.
  await new Promise(resolve => setTimeout(resolve, 23000));
  const waitingAfter = await fixtureStats();
  assert.ok(waitingAfter.phone_pending_frames - waitingBefore.phone_pending_frames > 1000, 'Pending callback must keep sending actual RTP across the waiting period');
  assert.ok(waitingAfter.phone_pending_audio - waitingBefore.phone_pending_audio > 25, 'Repeated callback prompt must be audible');
  assert.equal(waitingAfter.phone_ended, 1, 'Unconfirmed second callback must still be open');
  assert.equal(waitingAfter.phone_frames, beforeCallback.phone_frames, 'Callback audio must remain isolated throughout the confirmation wait');
  assert.equal((await events()).filter(e => e.event_type === 'phone.speaking').length, 0);
  // Drop only UCTP control. Accepted callback work must continue on the server,
  // without redialing or needing a browser command to complete confirmation.
  await page.evaluate(() => window.scenarioControlSocket.close());
  await fetch(`${fixture.http}/__scenario_phone_digit`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ digit: '1' }) });
  await poll(async () => (await events()).some(e => e.event_type === 'phone.speaking'), 'Confirmed callback did not replace browser audio');
  await expect(page.locator('[data-edge=phone]')).toHaveAttribute('data-state', 'speaking', { timeout: 15000 });
  await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state', 'retired');
  await expect(page.locator('#phone-status')).toContainText('Same Conversation, same Session');
  await poll(async () => { const s = await fixtureStats(); return s.phone_frames > 10 && s.owner_received_frames > 10; }, 'Actual phone audio required in both directions');
  const phoneSpeaking = (await events()).find(e => e.event_type === 'phone.speaking');
  assert.equal(phoneSpeaking.payload.sid, organizerCall.sid);
  assert.equal(phoneSpeaking.payload.participant_id, roleIds.owner);
  assert.equal(phoneSpeaking.payload.details.retained_connid, organizerCall.connid);
  assert.equal(phoneSpeaking.payload.details.retired_connid, browserJoined.payload.connid);
  assert.equal(phoneSpeaking.payload.details.join_confirmed, true);
  await new Promise(resolve => setTimeout(resolve, 500));
  const stable = await fixtureStats();
  await new Promise(resolve => setTimeout(resolve, 500));
  const phoneAudio = await fixtureStats();
  assert.equal(phoneAudio.human_frames, stable.human_frames, 'Retired browser must no longer be audible');
  assert.ok(phoneAudio.phone_frames > stable.phone_frames && phoneAudio.owner_received_frames > stable.owner_received_frames);
  assert.equal(phoneAudio.phone_answered, 2); assert.equal(phoneAudio.ended, false);
  await page.screenshot({ path: `test-results/${screenshotPrefix}-phone-move.png`, fullPage: true });
  await pauseForRecording(3000);
  await page.locator('#end').click();
  await poll(() => page.locator('#proposal').isVisible(), 'Assistant did not propose arrangements after the owner ended voice');
  if (livePlanner) {
    for (const fact of ['RD742', 'Jonathan', 'Alex']) await expect(page.locator('#proposal-text')).toContainText(fact);
    await expect(page.locator('#proposal-text')).toContainText(voiceOnly ? /11:00|11\s*a\.?m\.?/i : /16:00|4\s*p\.?m\.?/i);
    await expect(page.locator('#proposal-text')).toContainText(voiceOnly ? /16:00|4\s*p\.?m\.?/i : /17:00|5\s*p\.?m\.?/i);
    await expect(page.locator('#proposal-text')).toContainText(/terminal C/i);
    await expect(page.locator('#proposal-text')).toContainText(/sandbox/i);
  } else await expect(page.locator('#proposal-text')).toHaveText(scenario.proposal);
  await pauseForRecording(3000);
  await page.getByRole('button', { name: 'Approve these sandbox arrangements' }).click();
  await expect(page.locator('#mission-updates')).toHaveText(voiceOnly ? 'Voice rehearsal complete · SMS deferred' : '4/4 final updates sent · 0/4 delivered', { timeout: plannerTimeout });
  await pauseForRecording(2000);
  const finalEvent = page.locator('.event').filter({ hasText: voiceOnly ? 'David → Jonathan · chat accepted' : 'David → Organizer · sms accepted' }).last();
  await finalEvent.click();
  await expect(page.locator('#evidence')).toContainText('"type": "message.send"');
  await expect(page.locator('#evidence')).toContainText(voiceOnly ? 'travel.voice_complete' : '[Sandbox arrangements]');
  await expect(page.locator('#evidence')).not.toContainText(fixture.assistant_token);
  await expect(page.locator('#network-title')).toContainText('Connections at event #');
  await page.screenshot({ path: `test-results/${screenshotPrefix}-reveal.png`, fullPage: true });
  await pauseForRecording(5000);
  if (recording) {
    await page.getByText('Where this goes next', { exact: true }).click();
    await pauseForRecording(5000);
  }
  if (workerError) throw workerError;
  snapshot = await events();
  assert.equal(snapshot.filter(e => e.event_type === 'session.invited').length, 2);
  assert.equal(snapshot.filter(e => e.event_type === 'browser.speaking').length, 1);
  const handoff = snapshot.find(e => e.event_type === 'browser.speaking');
  assert.equal(handoff.payload.details.retained_connid, organizerCall.connid);
  assert.ok(snapshot.every(e => e.cid === fixture.cid));
  assert.equal(actions.filter(a => a.type === 'session.invite').length, 2);
  assert.equal(actions.filter(a => a.type === 'message.send' && a.payload.delivery === 'sms').length, voiceOnly ? 0 : 5);
  assert.ok(actions.every(a => a.cid === fixture.cid));
  const evidence = await collectEvidence(assistant, fixture.cid, { mode: liveVoice ? 'live-vapi-fixture-sms'
    : livePlanner ? 'live-planner-fixture-voice-sms' : 'local-fixture' });
  evidence.mode_source = 'test-harness';
  evidence.measurements = { audio: measured, phone_audio: phoneAudio, retained_connection: organizerCall.connid,
    voice_only: voiceOnly, sms: voiceOnly ? 'deferred; zero submissions' : 'fixture' };
  for (const action of actions) assert.ok(evidence.commands.some(c => c.request.id === action.id && c.response), 'Every assistant action must have exported command evidence');
  const serialized = JSON.stringify(evidence);
  for (const secret of [fixture.owner_token, fixture.assistant_token, process.env.VAPI_PRIVATE_KEY].filter(Boolean)) assert.ok(!serialized.includes(secret), 'Export must not contain credentials');
  evidencePath = `test-results/${screenshotPrefix}-${fixture.cid}-evidence.json`;
  await saveEvidence(evidencePath, evidence);
  console.log(JSON.stringify({ event: 'complete.scenario.proved', mode: liveVoice
    ? 'live Vapi planning and voice; synthetic local SIP participants; fixture SMS' : livePlanner
    ? 'live Vapi planning; fixture voice/SMS; real local browser/SIP media' : 'local providers and real browser/SIP media', cid: fixture.cid,
    sip_calls: 4, callback_attempts: 2, phone_move_verified: true, phone_audio: phoneAudio, voice_only: voiceOnly, individually_addressed_final_updates: voiceOnly ? 0 : 4, sms_delivered_claimed: 0, retained_connection: organizerCall.connid,
    uctp_actions: actions.length, planner_decisions: decisions, audio: measured }));
  scenarioPassed = true;
} finally {
  stop = true; if (run) await run;
  if (!scenarioPassed && !failureEvidenceSaved && assistant.authenticated) {
    await mkdir('test-results', { recursive: true });
    await saveEvidence(`test-results/${screenshotPrefix}-${fixture.cid}-failure.json`, {
      failure: workerError?.message || 'Scenario did not complete',
      worker: { cursor: worker.state?.cursor, decision_rejections: worker.state?.decisionRejections,
        pending_operations: worker.state?.pending?.requests.map(request => request.type) },
      ...(await collectEvidence(assistant, fixture.cid, { mode: liveVoice ? 'live-vapi-fixture-sms' : livePlanner ? 'live-planner-fixture-voice-sms' : 'local-fixture' })),
    });
  }
  assistant.close();
  ownerCheck.close();
  try {
    await context?.close();
    if (recording && video && scenarioPassed) {
      const output = `${recordingDir}/rehearsal-silent.webm`;
      await video.saveAs(output); await video.delete(); await chmod(output, 0o600);
      assert.ok((await stat(output)).size > 10000, 'Recording must contain video data');
      const sha256 = async path => createHash('sha256').update(await readFile(path)).digest('hex');
      await saveEvidence(`${recordingDir}/recording.json`, {
        schema: 'parley.conference-recording/1', cid: fixture.cid, silent: true,
        description: 'Recorded automated rehearsal; synthetic participants; fixture SMS. Presenter narration required.',
        mode: liveVoice ? 'live-vapi-fixture-sms' : livePlanner ? 'live-planner-fixture-voice-sms' : 'local-fixture',
        validation: 'Browser scenario passed; consult the outer Rust gate for provider and SIP teardown results.',
        video: { path: output, sha256: await sha256(output), width: 1600, height: 1100 },
        evidence: { path: evidencePath, sha256: await sha256(evidencePath) },
      });
      console.log(JSON.stringify({ event: 'conference.recording.saved', path: output, silent: true }));
    }
  } finally { await browser.close(); plannerApi.close(); }
}
