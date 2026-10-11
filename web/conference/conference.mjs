import { UctpClient } from '/uctp-client/client.mjs';
import { BrowserAudio } from '/uctp-client/browser-audio.mjs';
import { ConversationProjection, stageText } from './projection.mjs';
import { renderNetwork } from './network.mjs';
import { BrowserRingtone } from './ringtone.mjs';

const $ = id => document.getElementById(id);
let client, audio, cid, me, assistant, projection, capabilities, selectedRoute, selectedSeq, cursor = 0, activeSession = null, interruptedSession = null, proposal = null, polling = false, voiceOnly = false;
const members = new Map(); const events = []; const traces = new Map(); const channels = new Map();
const ringtone = new BrowserRingtone();
let joining = false;
const error = value => { $('notice').textContent = value?.message || String(value); };
const name = id => stageText(members.get(id)?.name || id || 'Participant');
const messageId = () => `msg_${crypto.randomUUID().replaceAll('-', '')}`;
const params = new URL(location.href).searchParams;
if (params.get('cid')) $('cid').value = params.get('cid');
if (params.get('uctp')) $('url').value = params.get('uctp');
else if (location.protocol === 'https:') $('url').value = `wss://${location.host}/uctp`;
if (params.get('mode') === 'voice-only') $('task').value = "My flight from Reno to Atlanta was canceled. Please ask the travel booker whether the reservations for Alex and me can be changed to a replacement flight with two seats. Aim to depart around 11 AM and arrive by 4 PM. This is a sandbox change only, with no real purchase. Then call the conference organizer directly to confirm pickup using the booker's actual answer. Bring me into that same call from my browser so I can move to my phone. Leave the call open until I end it, then ask me to approve the final sandbox arrangements. SMS is deferred until campaign approval; do not send any texts.";

function trace(direction, frame) {
  if (frame.type.startsWith('auth.') || frame.type.startsWith('conversation.')) return;
  const id = direction === 'send' ? frame.id : frame.in_reply_to;
  if (!id) return;
  const pair = traces.get(id) || {}; pair[direction === 'send' ? 'request' : 'response'] = frame; traces.set(id, pair);
}

$('setup').addEventListener('submit', async event => {
  event.preventDefault(); $('notice').textContent = '';
  await ringtone.unlock(); ringtone.stop(); joining = false;
  try {
    client?.close(); audio?.closeLocal(); activeSession = null; interruptedSession = null; $('recovery').classList.add('hidden'); $('join').disabled = true; $('end').disabled = true; cid = $('cid').value.trim(); cursor = 0; events.length = 0; traces.clear(); channels.clear();
    client = new UctpClient($('url').value.trim(), $('token').value, { trace, timeoutMs: 20000 });
    await client.connect();
    const first = await client.snapshot(cid);
    projection = new ConversationProjection(cid, first.participants); selectedRoute = null; selectedSeq = null;
    $('timeline').replaceChildren(); $('evidence').textContent = 'No event selected.';
    proposal = null; $('proposal').classList.add('hidden');
    members.clear(); first.participants.forEach(m => members.set(m.participant_id, m));
    voiceOnly = first.participants.filter(m => m.role !== 'assistant').every(m => !m.sms);
    me = first.participants.find(m => m.subject === client.identity);
    assistant = first.participants.find(m => m.role === 'assistant');
    if (me?.role !== 'owner' || !assistant) throw new Error('Use the owner participant token for this conference view');
    audio = new BrowserAudio(client, cid, $('remote-audio'));
    $('setup').classList.add('hidden'); $('token').value = ''; document.body.classList.add('presenting'); $('presentation-view').textContent = 'Scroll view';
    $('status').textContent = voiceOnly ? 'Connected · Voice rehearsal · SMS deferred' : first.capabilities.sms_mode === 'fake' ? 'Connected · SMS fixture' : 'Connected · Telnyx SMS';
    $('status').className = `status ${first.capabilities.sms_mode === 'fake' ? 'fake' : 'ready'}`;
    $('assistant-state').textContent = `${assistant.name} · waiting for external worker activity`;
    $('correlation').textContent = `CONVERSATION  ${cid}`;
    consume(first); renderPeople();
    if (!polling) { polling = true; poll(); }
  } catch (e) { client?.close(); error(e); }
});

async function poll() {
  try {
    if (client && !client.authenticated) await client.connect();
    if (client) consume(await client.snapshot(cid, cursor));
  } catch (e) {
    ringtone.stop(); $('join').disabled = true;
    $('browser-invitation').classList.add('hidden');
    $('status').textContent = 'Connection interrupted · recovering'; renderStage(false); error(e);
  }
  setTimeout(poll, 750);
}

function consume(snapshot) {
  capabilities = snapshot.capabilities;
  const needsSmsReview = !voiceOnly && capabilities.sms_enrollment_required;
  const eligible = new Set((snapshot.sms_eligibility || []).filter(e => e.eligible).map(e => e.participant_id));
  const smsReady = !needsSmsReview || (['owner', 'organizer'].every(role => snapshot.participants.some(m => m.role === role && m.sms && eligible.has(m.participant_id)))
    && snapshot.participants.filter(m => m.sms).every(m => eligible.has(m.participant_id)));
  $('send').disabled = snapshot.state === 'closed' || !smsReady;
  const build = capabilities.implementation;
  $('build-info').textContent = build
    ? `Rvoip ${stageText(build.rvoip_baseline)}${build.rvoip_patched ? ' + conference patches' : ''} · ${stageText(build.profile)} · UCTP v${build.envelope_version} / ${stageText(build.control_transport)}${build.experimental ? ' · experimental' : ''}`
    : 'Server build details unavailable';
  $('status').textContent = voiceOnly ? 'Connected · Voice rehearsal · SMS deferred' : snapshot.capabilities.sms_mode === 'fake' ? 'Connected · SMS fixture' : 'Connected · Telnyx SMS';
  if (!smsReady) $('status').textContent = 'Connected · SMS enrollment review required before Start';
  for (const event of snapshot.events) {
    if (event.seq <= cursor) continue;
    if (!projection.apply(event)) continue;
    cursor = event.seq; events.push(event);
    const p = event.payload;
    if (p.from === assistant.participant_id) $('assistant-state').textContent = `${assistant.name} · activity received through UCTP`;
    if (event.event_type === 'session.invited') { activeSession = p.sid; $('end').disabled = false; }
    if (event.event_type === 'session.interrupted') {
      activeSession = p.sid; interruptedSession = p.sid; audio?.closeLocal();
      $('join').disabled = true; $('end').disabled = true;
      $('recovery').classList.remove('hidden'); $('verify-ended').disabled = false;
      $('assistant-state').textContent = 'Assistant paused · verify the interrupted call';
    }
    if (['session.ended', 'session.failed'].includes(event.event_type) && p.sid === activeSession) {
      if (interruptedSession === p.sid) $('assistant-state').textContent = `${assistant.name} · recovery verified; waiting for worker activity`;
      const ended = projection.sessions.get(p.sid);
      if (ended?.participant) channels.set(ended.participant, `Voice · ${ended.state}`);
      if (ended?.browserConnection) channels.set(me.participant_id, `WebRTC · ${ended.state}`);
      activeSession = null; interruptedSession = null; $('recovery').classList.add('hidden'); $('join').disabled = true; $('end').disabled = true; audio?.closeLocal();
    }
    if (event.event_type === 'message.accepted' && p.content_type === 'application/json') {
      try {
        const content = JSON.parse(p.body);
        if (content.type === 'travel.proposal' && p.from === assistant.participant_id) {
          proposal = content; $('proposal-title').textContent = 'Ready for your approval'; $('approve').textContent = 'Approve these sandbox arrangements'; $('proposal-text').textContent = stageText(content.summary); $('proposal').classList.remove('hidden'); $('approve').disabled = false;
        }
        if (content.type === 'travel.approval' && p.from === me.participant_id && content.approved === true && content.version === 1 && content.proposal_id === proposal?.id) { $('approve').disabled = true; $('approve').textContent = 'Approved'; $('proposal-title').textContent = 'Approved sandbox arrangements'; }
      } catch {}
    }
    if (event.event_type === 'message.accepted') for (const recipient of p.to || []) channels.set(recipient, `${p.delivery.toUpperCase()} · accepted`);
    if (event.event_type === 'message.delivery') channels.set(p.participant_id, `SMS · ${p.state}`);
    if (event.event_type === 'message.received') channels.set(p.from, 'SMS · replied');
    if (event.event_type.startsWith('connection.') && p.participant_id) channels.set(p.participant_id, `${p.transport || 'voice'} · ${p.state}`);
    if (event.event_type === 'browser.speaking') channels.set(me.participant_id, 'WebRTC ↔ telephone');
    if (event.event_type.startsWith('phone.')) {
      if (p.state === 'speaking') { channels.set(me.participant_id, 'Telephone ↔ telephone · same Conversation'); audio?.closeLocal(); }
      if (['failed', 'cancelled', 'ended'].includes(p.state)) audio?.resetPhoneMove();
    }
    addEvent(event);
  }
  const current = projection.voice;
  const invitation = snapshot.state === 'closed' ? null : projection.browserInvitation;
  $('join').disabled = joining || !capabilities.browser_handoff || !invitation;
  $('join').textContent = joining ? 'Joining organizer call…' : invitation ? 'Answer organizer call' : 'Waiting for organizer call';
  $('browser-invitation').classList.toggle('hidden', !invitation || joining);
  $('browser-invitation').textContent = invitation ? 'Organizer call ready—answer here to join Jonathan from this browser.' : '';
  ringtone.start(joining ? null : invitation?.sid);
  const phonePending = ['prepared', 'dialing', 'answered', 'confirmed', 'committing'].includes(current?.phone);
  $('move-phone').disabled = snapshot.state === 'closed' || !capabilities.phone_handoff || !me.sip || current?.browser !== 'speaking' || phonePending || current?.phone === 'unknown';
  $('cancel-phone').classList.toggle('hidden', !phonePending);
  $('cancel-phone').disabled = !phonePending || current?.phone === 'committing';
  $('phone-status').textContent = phonePending ? 'Your phone is being called. Answer and press 1 to join; your browser audio stays connected until the move succeeds.'
    : current?.phone === 'speaking' ? 'You’re on your phone. Same Conversation, same Session, organizer’s call retained.'
    : current?.phone === 'unknown' ? 'Phone move needs reconciliation. Check the actual call before retrying.'
    : ['failed', 'cancelled'].includes(current?.phone) ? 'Phone move did not complete. Browser audio retained if still available.' : '';
  if (snapshot.state === 'closed') {
    $('status').textContent = 'Conversation closed · history retained';
    for (const id of ['send', 'join', 'end', 'approve', 'verify-ended', 'move-phone', 'cancel-phone']) $(id).disabled = true;
    $('assistant-state').textContent = 'Assistant stopped · Conversation closed';
    audio?.closeLocal();
  }
  renderPeople(); renderStage(true);
}

function renderPeople() {
  $('people').replaceChildren();
  for (const m of members.values()) {
    if (m.role === 'assistant') continue;
    const card = document.createElement('div'); card.className = 'person';
    const initial = document.createElement('div'); initial.className = 'initial'; initial.textContent = m.name[0];
    const title = document.createElement('strong'); title.textContent = stageText(m.name);
    const role = document.createElement('small'); role.textContent = m.role;
    const channel = document.createElement('div'); channel.className = 'channel'; channel.textContent = channels.get(m.participant_id) || 'Not contacted yet';
    card.append(initial, title, role, channel); $('people').append(card);
  }
}

function addEvent(event) {
  $('timeline').querySelector('.empty')?.remove();
  const p = event.payload; const button = document.createElement('button'); button.className = 'event';
  let label = event.event_type.replaceAll('.', ' · ');
  if (event.event_type === 'session.speech') label = `${name(p.speaker)} · ${p.state === 'started' ? 'speaking' : 'finished speaking'}`;
  if (event.event_type === 'message.accepted') label = `${name(p.from)} → ${(p.to || []).map(name).join(', ')} · ${p.delivery} accepted`;
  if (event.event_type === 'message.delivery') label = `${name(p.participant_id)} · SMS ${p.state}`;
  if (event.event_type === 'message.received') label = `${name(p.from)} replied: ${p.body}`;
  if (event.event_type === 'session.transcript') label = `${name(p.speaker)}: ${p.text}`;
  if (event.event_type === 'browser.speaking') label = 'Jonathan joined · telephone connection retained';
  if (event.event_type === 'phone.speaking') label = 'Moved to Jonathan’s phone · same Conversation, telephone connection retained';
  button.append(document.createTextNode(stageText(label)));
  const detail = document.createElement('small'); detail.textContent = `#${event.seq} · ${new Date(event.created_at).toLocaleTimeString()} · ${event.request_id || 'inbound event'}`; button.append(detail);
  button.dataset.seq = String(event.seq);
  button.onclick = () => inspectEvent(event.seq);
  $('timeline').append(button); $('timeline').scrollTop = $('timeline').scrollHeight;
}

async function inspectEvent(seq) {
  const event = events.find(e => e.seq === seq); if (!event) return;
  $('timeline').classList.add('hidden'); $('wire-panel').classList.remove('hidden'); $('show-timeline').classList.remove('hidden');
  $('activity-title').textContent = 'One interface. See the request.';
  document.querySelectorAll('.event.selected').forEach(e => e.classList.remove('selected'));
  $('timeline').querySelector(`[data-seq="${seq}"]`)?.classList.add('selected');
  selectedSeq = seq; selectedRoute = projection.routeFor(event); renderStage(client.authenticated);
  let pair = traces.get(event.request_id);
  if (!pair && event.request_id) {
    try { pair = (await client.request(client.command('conversation.inspect', cid, { request_id: event.request_id }))).payload.evidence; }
    catch { /* Provider-originated events may have no client request. */ }
  }
  $('evidence-caption').textContent = pair?.request ? 'Actual client request, correlated reply, and durable server event.' : 'Actual event received through UCTP. This client did not originate the underlying request.';
  $('evidence').textContent = stageText(JSON.stringify({ ...(pair || {}), observed_event: event }, null, 2));
}

function renderStage(connected) {
  if (!projection) return;
  let view = projection;
  if (selectedSeq) {
    view = new ConversationProjection(cid, [...members.values()]);
    for (const event of events) { if (event.seq > selectedSeq) break; view.apply(event); }
  }
  $('network-title').textContent = selectedSeq ? `Connections at event #${selectedSeq}` : 'One Conversation, across the connections';
  $('show-live').classList.toggle('hidden', !selectedSeq);
  const network = view.network(selectedSeq ? undefined : connected, voiceOnly ? 'deferred' : capabilities?.sms_mode);
  renderNetwork($('network'), network, selectedRoute, inspectEvent);
  const voice = view.voice;
  $('network-ids').textContent = voice ? `Session ${voice.sid} · remote Connection ${voice.remote}${voice.phoneConnection ? ` · owner phone ${voice.phoneConnection}` : ''}` : 'No voice Session has been invited.';
  $('retained').textContent = network.retained ? `Same telephone Connection retained at handoff${['ended', 'failed', 'interrupted'].includes(voice.state) ? ` · Session ${voice.state}` : ''}` : 'Speaking-peer replacement will retain the existing telephone Connection.';
  $('retained').classList.toggle('proved', network.retained);
  $('mission-task').textContent = projection.task ? stageText(projection.task.payload.body.length > 180 ? `${projection.task.payload.body.slice(0, 177)}…` : projection.task.payload.body) : 'Waiting for Jonathan’s task';
  $('mission-approval').textContent = projection.approval ? 'Owner approved sandbox arrangements' : projection.proposal ? 'Waiting for owner approval' : 'Arrangements still being gathered';
  const updates = projection.finalUpdates();
  const count = states => updates.filter(u => states.includes(u.delivery?.state)).length;
  const smsTotal = [...projection.members.values()].filter(m => m.role !== 'assistant' && m.sms).length;
  const chats = updates.filter(u => u.chat && !u.member.sms).length;
  $('mission-updates').textContent = voiceOnly ? projection.voiceComplete ? 'Voice rehearsal complete · SMS deferred' : 'SMS deferred · voice rehearsal only' : `${count(['sent', 'delivered'])}/${smsTotal} final updates sent · ${count(['delivered'])}/${smsTotal} delivered${smsTotal < 4 ? ` · ${chats}/${4 - smsTotal} chat updates accepted` : ''}`;
  const uncertain = count(['unknown', 'failed']);
  $('mission-update-note').textContent = voiceOnly ? 'Text messages resume after campaign approval and delivery checks.' : uncertain ? `${uncertain} update(s) need attention` : projection.approval ? 'Sent and delivered are separate provider outcomes.' : 'Final updates wait for owner approval.';
  $('facts').replaceChildren();
  for (const fact of projection.facts.slice(-3)) {
    const item = document.createElement('button'); item.className = 'fact';
    const p = fact.payload;
    item.textContent = stageText(`${name(p.speaker || p.from)} · ${fact.event_type === 'session.transcript' ? 'final voice transcript' : 'SMS reply'}: ${p.text || p.body}`);
    item.onclick = () => inspectEvent(fact.seq); $('facts').append(item);
  }
  $('facts-empty').classList.toggle('hidden', projection.facts.length > 0);
}

async function send(body, content_type = 'text/plain') {
  return client.request(client.command('message.send', cid, { msg_id: messageId(), to: [assistant.participant_id], body, content_type, delivery: 'chat' }));
}
$('send').onclick = async () => {
  await ringtone.unlock();
  $('send').disabled = true;
  try { await send($('task').value); $('send').textContent = 'Send follow-up'; $('task-composer').open = false; }
  catch (e) { error(e); }
  finally { $('send').disabled = false; }
};
$('approve').onclick = async () => {
  if (!proposal) return; $('approve').disabled = true;
  try { await send(JSON.stringify({ type: 'travel.approval', version: 1, proposal_id: proposal.id, approved: true }), 'application/json'); }
  catch (e) { error(e); $('approve').disabled = false; }
};
$('join').onclick = async () => {
  const invitation = projection?.browserInvitation;
  if (!invitation || joining || activeSession !== invitation.sid) return;
  ringtone.answer(invitation.sid); joining = true; $('browser-invitation').classList.add('hidden'); $('join').disabled = true;
  try { await audio.join(activeSession); }
  catch (e) { error(e); }
  finally { joining = false; }
};
$('end').onclick = async () => {
  if (!activeSession) return; $('end').disabled = true;
  try { await client.request(client.command('session.end', cid, {}, { sid: activeSession })); audio.closeLocal(); }
  catch (e) { error(e); $('end').disabled = false; }
};

$('move-phone').onclick = async () => {
  $('move-phone').disabled = true;
  try { await audio.moveToPhone(); }
  catch (e) { error(e); $('move-phone').disabled = false; }
};
$('cancel-phone').onclick = async () => {
  $('cancel-phone').disabled = true;
  try { await audio.cancelPhoneMove(projection.voice?.phoneConnection); }
  catch (e) { error(e); }
};

$('verify-ended').onclick = async () => {
  if (!interruptedSession) return;
  const note = $('verification-note').value.trim();
  if (!note) { error('Describe how you verified the remote call has ended.'); return; }
  $('verify-ended').disabled = true;
  try {
    await client.request(client.command('session.update', cid,
      { kind: 'confirm_ended', verification_note: note }, { sid: interruptedSession }));
    $('verification-note').value = '';
  } catch (e) { error(e); $('verify-ended').disabled = false; }
};

$('community').addEventListener('toggle', () => { $('future-connectors').classList.toggle('hidden', !$('community').open); $('contribution-panel').classList.toggle('expanded', $('community').open); });

$('show-live').onclick = () => { selectedSeq = null; selectedRoute = null; renderStage(client?.authenticated === true); };

$('show-timeline').onclick = () => {
  $('wire-panel').classList.add('hidden'); $('timeline').classList.remove('hidden'); $('show-timeline').classList.add('hidden');
  $('activity-title').textContent = 'The Conversation, as it happens';
  selectedSeq = null; selectedRoute = null; renderStage(client?.authenticated === true);
};
renderNetwork($('network'), new ConversationProjection('', []).network(false, undefined), null, () => {});

$('presentation-view').onclick = () => { const active = document.body.classList.toggle('presenting'); $('presentation-view').textContent = active ? 'Scroll view' : 'Room view'; };
