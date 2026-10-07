// Read-only checks: no provider calls, messages, resource provisioning, or test dial.
import { withOwner } from './conference-operator.mjs';
const [bundlePath, mode = 'local'] = process.argv.slice(2);
try {
  if (!bundlePath || !['local', 'live', 'voice-only'].includes(mode)) throw new Error('Usage: scripts/preflight-conference.sh provisioned.json [local|live|voice-only]');
  await withOwner(bundlePath, async (client, bundle) => {
    const { payload } = await client.request(client.command('conversation.preflight', bundle.cid));
    const problems = [];
    if (!payload.readiness?.ready_for_new_task) problems.push('Conversation is closed, has unsettled work, or shares SMS endpoints with another open Conversation');
    const members = (await client.snapshot(bundle.cid)).participants;
    for (const role of ['owner', 'companion', 'booker', 'organizer']) {
      const found = members.filter(p => p.role === role);
      if (found.length !== 1 || (mode !== 'voice-only' && !found[0].sms)) problems.push(`${role}: one participant${mode === 'voice-only' ? '' : ' with an SMS endpoint'} required`);
      if (mode === 'voice-only' && found.some(p => p.sms)) problems.push(`${role}: remove SMS endpoints from the voice-only roster`);
    }
    if (mode === 'live' || mode === 'voice-only') {
      if (mode === 'live' && payload.capabilities?.sms_mode !== 'telnyx') problems.push('Server SMS mode is not Telnyx');
      for (const capability of [...(mode === 'live' ? ['sms_configured'] : []), 'voice', 'assistant_voice', 'browser_handoff']) {
        if (!payload.capabilities?.[capability]) problems.push(`${capability} is not available`);
      }
      for (const role of ['booker', 'organizer']) if (!members.find(p => p.role === role)?.sip) problems.push(`${role}: provisioned SIP route required`);
      if (payload.capabilities?.phone_handoff && !members.find(p => p.role === 'owner')?.sip) problems.push('owner: provisioned callback SIP route required for Move to my phone');
      if (mode === 'voice-only' && !payload.capabilities?.phone_handoff) problems.push('phone_handoff is not available');
    } else if (payload.capabilities?.sms_mode !== 'fake') problems.push('Local preflight requires fake SMS mode');
    console.log(JSON.stringify({ cid: bundle.cid, check: 'configuration and local state only', mode,
      readiness: payload.readiness, capabilities: payload.capabilities, problems,
      unverified: ['provider credentials and callbacks', 'carrier delivery', 'SIP/PSTN reachability', 'two-way audio and conference network'] }, null, 2));
    if (problems.length) process.exitCode = 1;
  });
} catch (error) { console.error(error.message); process.exitCode = 1; }
