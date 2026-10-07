// Explicitly invoked AWS rehearsal support. Never logs credentials.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const exec = promisify(execFile);
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
export async function ssm(commands) {
  const args = ['--profile', 'vapi-admin', '--region', 'us-west-2', 'ssm'];
  const instance = 'i-08be047e467a7996c';
  const sent = JSON.parse((await exec('aws', [...args, 'send-command', '--instance-ids', instance,
    '--document-name', 'AWS-RunShellScript', '--parameters', JSON.stringify({commands: ['set -eu', ...commands]}), '--output', 'json'])).stdout);
  const id = sent.Command.CommandId;
  for (let i = 0; i < 25; i++) {
    await pause(1000);
    let result;
    try { result = JSON.parse((await exec('aws', [...args, 'get-command-invocation', '--command-id', id, '--instance-id', instance, '--output', 'json'])).stdout); }
    catch (error) { if (error.stderr?.includes('InvocationDoesNotExist')) continue; throw error; }
    if (['Pending', 'InProgress', 'Delayed'].includes(result.Status)) continue;
    if (result.Status !== 'Success') throw new Error(`SSM ${id}: ${result.Status}: ${result.StandardErrorContent}`);
    return result.StandardOutputContent;
  }
  throw new Error(`SSM ${id} still running; inspect before retrying`);
}
export async function vapiCalls(fixture) {
  const res = await fetch(`https://api.vapi.ai/call?assistantId=${encodeURIComponent(process.env.VAPI_ASSISTANT_ID)}&limit=30`, {
    headers: {authorization: `Bearer ${process.env.VAPI_PRIVATE_KEY}`}, signal: AbortSignal.timeout(10000)});
  if (!res.ok) throw new Error(`Vapi status ${res.status}`);
  const matching = (await res.json()).filter(call => call.metadata?.conversation_id === fixture.cid && call.metadata?.session_id === fixture.sid);
  // The list endpoint can lag call finalization; verify each exact call record.
  return Promise.all(matching.map(async call => {
    const detail = await fetch(`https://api.vapi.ai/call/${encodeURIComponent(call.id)}`, {
      headers: {authorization: `Bearer ${process.env.VAPI_PRIVATE_KEY}`}, signal: AbortSignal.timeout(10000)});
    if (!detail.ok) throw new Error(`Vapi call status ${detail.status}`);
    const record = await detail.json();
    if (record.metadata?.conversation_id !== fixture.cid || record.metadata?.session_id !== fixture.sid) throw new Error('Vapi correlation mismatch');
    return record;
  }));
}
export async function sipProbe(fixture) {
  if (!/^\/opt\/parley\/state\/webrtc-probe-[a-z0-9-]+\.json$/.test(fixture.statusFile)) throw new Error('Invalid status path');
  return JSON.parse(await ssm([`cat ${fixture.statusFile}`]));
}
export async function cloudProbe(fixture) {
  const state = await sipProbe(fixture);
  return {sip_ended: state.state === 'ended', browser_audio_frames: state.browser_tone_frames,
    browser_first_audio_ms: state.first_browser_tone_ms, received_frames: state.received_frames};
}

// Provider status can lag actual termination by >30 seconds. Check final
// provider timestamps against the journal, without keeping the SIP call open.
export async function verifyProviderRetirement(fixture, journal) {
  const handoff = journal.events.find(e => e.event_type === 'browser.speaking' && e.payload.sid === fixture.sid);
  const ended = journal.events.find(e => e.event_type === 'session.ended' && e.payload.sid === fixture.sid);
  if (!handoff || !ended || handoff.payload.details?.retained_connid !== fixture.connid) throw new Error('Retained SIP handoff/teardown missing');
  const calls = await verifyCallsEnded(fixture);
  if (!calls.every(call => Number.isFinite(Date.parse(call.endedAt)) && Date.parse(call.endedAt) < Date.parse(ended.created_at)))
    throw new Error('Provider must have ended before the retained SIP Session');
  return {calls, handoff_at:handoff.created_at, sip_ended_at:ended.created_at,
    provider_end_after_handoff_ms:calls.map(call => Date.parse(call.endedAt)-Date.parse(handoff.created_at)),
    verified_at:new Date().toISOString()};
}

export async function verifyCallsEnded(fixture) {
  const deadline = Date.now() + 90000;
  let calls = [], lastError;
  do {
    try { calls = await vapiCalls(fixture); lastError = null; }
    catch (error) { lastError = error; }
    if (calls.length && calls.every(call => call.status === 'ended' && Number.isFinite(Date.parse(call.endedAt)))) return calls;
    await pause(3000);
  } while (Date.now() < deadline);
  throw new Error(`Provider retirement remains unverified${lastError ? `: ${lastError.message}` : ''}`);
}
