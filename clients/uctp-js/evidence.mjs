import { redact } from './client.mjs';

export const EVIDENCE_MODES = ['local-fixture', 'live-planner-fixture-voice-sms', 'live-vapi-fixture-sms', 'live-providers'];

// Read only. The journal and command records remain authoritative; no events or
// delivery outcomes are synthesized to fill gaps in an exported run.
export async function collectEvidence(client, cid, { mode, maxPages = 100 } = {}) {
  if (!EVIDENCE_MODES.includes(mode)) throw new Error('An explicit rehearsal mode is required');
  if (!Number.isInteger(maxPages) || maxPages < 1 || maxPages > 100) throw new Error('Invalid evidence page limit');
  const events = []; let after = 0, snapshot, drained = false;
  for (let page = 0; page < maxPages; page++) {
    snapshot = await client.snapshot(cid, after);
    if (!Array.isArray(snapshot.events) || snapshot.events.length > 500) throw new Error('Invalid journal page');
    for (const event of snapshot.events) {
      if (event.cid !== cid || !Number.isSafeInteger(event.seq) || event.seq <= after) throw new Error('Foreign or nonadvancing journal event');
      events.push(event); after = event.seq;
    }
    if (snapshot.events.length < 500) { drained = true; break; }
  }
  if (!drained) throw new Error('Evidence page limit reached; no complete export was produced');
  const self = snapshot.participants.find(p => p.subject === client.identity);
  if (!self || !['owner', 'assistant'].includes(self.role)) throw new Error('Owner or assistant scope required for full task evidence');
  if (snapshot.capabilities.sms_mode !== (mode === 'live-providers' ? 'telnyx' : 'fake')) throw new Error('Declared mode conflicts with server SMS mode');
  const commands = [], unavailable = [];
  for (const request_id of new Set(events.map(e => e.request_id).filter(Boolean))) {
    let evidence;
    try {
      evidence = (await client.request(client.command('conversation.inspect', cid, { request_id }))).payload.evidence;
    } catch (error) {
      // External callbacks and legacy facts may not have a client command;
      // ambiguous IDs remain explicit gaps rather than being silently matched.
      if (![404, 409].includes(error.code)) throw error;
      unavailable.push({ request_id, code: error.code }); continue;
    }
    if (evidence?.request?.cid !== cid || evidence.request.id !== request_id
      || (evidence.response && (evidence.response.in_reply_to !== request_id || evidence.response.cid !== cid))) throw new Error('Mismatched command evidence');
    commands.push({ actor_subject: evidence.actor_subject, request: redact(evidence.request),
      response: evidence.response ? redact(evidence.response) : null,
      event_seqs: events.filter(e => e.request_id === request_id).map(e => e.seq) });
  }
  // Reuse the inspector's credential/SDP redaction over the complete document.
  // Message bodies and transcripts are retained deliberately: this is a private
  // evidence file, not a guarantee that personal content is safe to publish.
  return redact({ type: 'conference.evidence', schema: 'parley.conference-evidence/1',
    captured_at: new Date().toISOString(), cid, mode, mode_source: 'operator-declared',
    publication: 'private; review message bodies, transcripts and endpoints before sharing',
    capture: { through_seq: after, drained: true, atomic_snapshot: false, state: snapshot.state },
    implementation: snapshot.capabilities.implementation, sms_mode: snapshot.capabilities.sms_mode,
    participants: snapshot.participants.map(({ participant_id, name, role }) => ({ participant_id, name, role })),
    events, commands, unavailable,
  });
}
