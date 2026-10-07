// Opt-in deployment gate: real AWS WebRTC + real Vapi, loopback synthetic SIP.
// Requires AWS vapi-admin and deployment secrets in the environment.
import {mkdir, writeFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {UctpClient, redact} from '../clients/uctp-js/client.mjs';
import {ssm, verifyProviderRetirement} from './support/cloud-probe.mjs';
const http = 'https://conference.rudeless.ai', url = 'wss://conference.rudeless.ai/uctp';
const speechHarness = process.argv[2] === '--speech';
if (process.argv.length > (speechHarness ? 3 : 2)) throw new Error('Usage: run-conference-cloud-media.mjs [--speech]');
const run = Date.now().toString(36), unit = `parley-webrtc-probe-${run}`;
const output = `var/conference/live/webrtc-${run}`;
const statusFile = `/opt/parley/state/webrtc-probe-${run}.json`;
await mkdir(output, {recursive: true, mode: 0o700});
const save = (name, value) => writeFile(`${output}/${name}.json`, JSON.stringify(value, null, 2), {mode:0o600});
const admin = new UctpClient(url, process.env.PARLEY_API_SECRET, {timeoutMs:20000});
let owner, assistant, fixture, routeCreated = false, peerStarted = false, failure;
async function request(client, name, command) {
  await save(`${name}-command`, command);
  const reply = await client.request(command); await save(`${name}-response`, redact(reply)); return reply;
}
try {
  // Bind only loopback, never dial a real recipient. The host-local route avoids
  // an EC2 Elastic IP hairpin solely for this synthetic SIP leg.
  const exists = (await ssm(['ip route show table local exact 32.185.99.169/32'])).trim();
  if (!exists) { await ssm(['ip route add local 32.185.99.169/32 dev lo']); routeCreated = true; }
  await save('cleanup', {unit, statusFile, routeCreated});
  await ssm([`systemd-run --unit=${unit} --property=User=parley --property=UMask=0077 --property=RuntimeMaxSec=420 /opt/parley/build/target/release/examples/conference_media_peer ${statusFile}`]);
  peerStarted = true;
  const state = JSON.parse(await ssm([`cat ${statusFile}`]));
  if (state.state !== 'listening') throw new Error('Synthetic SIP peer not listening');
  await admin.connect();
  const created = await request(admin, 'create', admin.command('conversation.create', null, {participants:[
    {alias:'jonathan', name:'Jonathan — network test', role:'owner'},
    {alias:'organizer', name:'Synthetic SIP tone endpoint', role:'organizer', sip:'sip:network-probe@127.0.0.1:5094'},
    {alias:'assistant', name:'Vapi assistant', role:'assistant'}]}));
  const participants = created.payload.participants;
  for (const member of participants.filter(p => ['owner', 'assistant'].includes(p.role))) {
    const res = await fetch(`${http}/v1/conference/${created.cid}/tokens`, {method:'POST', redirect:'error',
      headers:{authorization:`Bearer ${process.env.PARLEY_API_SECRET}`, 'content-type':'application/json'},
      body:JSON.stringify({participant_id:member.participant_id}), signal:AbortSignal.timeout(10000)});
    if (!res.ok) throw new Error(`Token provisioning: ${res.status}`);
    member.token = (await res.json()).token;
  }
  const ownerMember = participants.find(p => p.role === 'owner');
  owner = new UctpClient(url, ownerMember.token, {timeoutMs:20000});
  assistant = new UctpClient(url, participants.find(p => p.role === 'assistant').token, {timeoutMs:20000});
  await owner.connect(); await assistant.connect();
  fixture = {cid:created.cid, http, url, token:ownerMember.token, output, statusFile, publicIp:'32.185.99.169',
    syntheticSip:true, ...(speechHarness ? {speechPcm:'var/conference/pstn-speech/browser.pcm'} : {})};
  await save('fixture', fixture);
  const invited = await request(assistant, 'invite', assistant.command('session.invite', fixture.cid,
    {medium:'voice', to:participants.find(p => p.role === 'organizer').participant_id,
      purpose:'Synthetic audio connectivity test. Briefly introduce yourself, then remain on the line for a browser handoff. The other endpoint produces a test tone. Do not make external calls, send messages, or end this test call.'}));
  fixture.sid = invited.payload.session.sid; fixture.connid = invited.payload.session.connid;
  await save('fixture', fixture);
  console.log(JSON.stringify({event:'cloud.webrtc.start', cid:fixture.cid, sid:fixture.sid, output}));
  // Wait for actual provider attachment before opening the handoff controls.
  const until = Date.now()+30000;
  for (;;) {
    const snapshot = await owner.snapshot(fixture.cid);
    if (snapshot.events.some(e => e.event_type === 'session.assistant_attached' && e.payload.sid === fixture.sid)) break;
    if (Date.now() > until) { await save('before-browser', redact({type:'conference.snapshot', ...snapshot})); throw new Error('Vapi attachment did not become active'); }
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  // This transport probe has no planning worker. Publish a real scoped UCTP
  // invitation, as the worker does, rather than bypassing the browser's gate.
  await request(assistant, 'browser-invitation', assistant.command('message.send', fixture.cid, {
    msg_id: `msg_network_probe_${run}`, to: [ownerMember.participant_id],
    delivery: 'chat', content_type: 'application/json',
    body: JSON.stringify({type:'travel.browser_invitation', version:1,
      sid:fixture.sid, retained_connid:fixture.connid}),
  }));
  await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [speechHarness ? 'e2e/conference-pstn-handoff.mjs' : 'e2e/conference-cloud-media.mjs'], {stdio:'inherit', env:{...process.env,
      LIVE_VOICE_FIXTURE:JSON.stringify(fixture), PLAYWRIGHT_BROWSERS_PATH:'var/conference-browsers', PARLEY_BROWSER_CHANNEL:'chromium'}});
    child.on('error', reject); child.on('exit', code => code === 0 ? resolve() : reject(new Error(`Browser media gate exited ${code}`)));
  });
} catch(error) { failure = error; console.error(error.message); }
finally {
  if (owner?.authenticated && fixture) {
    try {
      // Always attempt teardown, including after a browser assertion fails.
      if (fixture.sid) await request(owner, 'cleanup-end', owner.command('session.end', fixture.cid, {}, {sid:fixture.sid}));
      const journal = await owner.snapshot(fixture.cid);
      await save('journal', redact({type:'conference.snapshot', ...journal}));
      const retirement = await verifyProviderRetirement(fixture, journal);
      await save('vapi-retirement', retirement);
      console.log(JSON.stringify({event:'cloud.vapi.retirement', provider_end_after_handoff_ms:retirement.provider_end_after_handoff_ms}));
    } catch(error) { console.error(`Session cleanup: ${error.message}`); failure ??= error; }
  }
  owner?.close(); assistant?.close(); admin.close();
  if (peerStarted) {
    try {
      try { await save('peer-final', JSON.parse(await ssm([`cat ${statusFile}`]))); }
      finally { await ssm([`if [ "$(systemctl show ${unit} --property=LoadState --value)" != not-found ]; then systemctl stop ${unit}; fi`]); }
    }
    catch(error) { console.error(`Peer cleanup: ${error.message}`); failure ??= error; }
  }
  if (routeCreated) {
    try { await ssm(['ip route del local 32.185.99.169/32 dev lo']); }
    catch(error) { console.error(`Route cleanup: ${error.message}`); failure ??= error; }
  }
}
await save('result', {passed:!failure, mode:speechHarness?'synthetic-sip-browser-speech':'synthetic-sip-browser-tones', pstn:false, error:failure?.message, completed_at:new Date().toISOString()});
if (failure) process.exitCode = 1;
else console.log(JSON.stringify({event:speechHarness?'cloud.speech_harness.passed':'cloud.webrtc.passed', output}));
