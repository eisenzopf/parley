import { createHash } from 'node:crypto';
import { mkdir, readFile, rename, writeFile, open, unlink } from 'node:fs/promises';
import { dirname } from 'node:path';
import { pathToFileURL } from 'node:url';
import { UctpClient, UctpError } from '../../clients/uctp-js/client.mjs';
import { VapiPlanner } from './vapi.mjs';
import { campaignSms } from './sms.mjs';

const digest = value => createHash('sha256').update(value).digest('hex').slice(0, 32);
const text = (value, limit = 16000) => typeof value === 'string' && value.trim().length > 0 && value.length <= limit;

export class FileState {
  constructor(path) { this.path = path; }
  async load(cid) {
    try {
      const state = JSON.parse(await readFile(this.path, 'utf8'));
      if (state.version !== 1 || state.cid !== cid) throw new Error('Worker state belongs to a different Conversation or version');
      return state;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      return { version: 1, cid, cursor: 0, events: [], pending: null, proposal: null, approved: null, completedProposal: null, needsDecision: false };
    }
  }
  async save(state) {
    await mkdir(dirname(this.path), { recursive: true, mode: 0o700 });
    const temp = `${this.path}.tmp`;
    await writeFile(temp, JSON.stringify(state), { mode: 0o600 });
    await rename(temp, this.path);
  }
}

/** One worker per Conversation. Its only communications dependency is UCTP. */
export class ConferenceWorker {
  constructor({ client, cid, planner, storage, mode = 'full', log = () => {}, now = () => Date.now() }) {
    if (!['full', 'voice-only'].includes(mode)) throw new Error('Unknown conference demo mode');
    this.client = client; this.cid = cid; this.planner = planner; this.storage = storage; this.log = log;
    this.now = now; this.mode = mode;
  }
  async init() {
    this.state = await this.storage.load(this.cid);
    const savedMode = this.state.demoMode || ((this.state.cursor || this.state.pending) ? 'full' : this.mode);
    if (savedMode !== this.mode) throw new Error('Worker mode changed; use a fresh Conversation and state file');
    this.state.demoMode = this.mode;
    await this.storage.save(this.state);
  }

  observe(newEvents) {
    const owner = this.members.find(m => m.role === 'owner');
    for (const event of newEvents) {
      if (event.seq <= this.state.cursor) continue;
      this.state.events.push(event); this.state.cursor = event.seq;
      if (event.event_type === 'session.assistant_action' && event.payload.action === 'finish_call') {
        this.state.finishObservedAt ??= {};
        this.state.finishObservedAt[event.seq] = this.now();
      }
      if (event.event_type === 'session.speech' && event.payload.speaker === this.self.participant_id) {
        this.state.assistantSpeechObservedAt ??= {};
        this.state.assistantSpeechObservedAt[event.payload.sid] = this.now();
      }
      if (['session.ended', 'session.failed', 'session.interrupted', 'connection.failed', 'session.assistant_failed'].includes(event.event_type)
        || (event.event_type === 'session.transcript' && event.payload.speaker !== this.self.participant_id)) this.state.needsDecision = true;
      if (event.event_type === 'session.speech' || (event.event_type === 'session.transcript' && event.payload.speaker !== this.self.participant_id)) {
        this.state.voiceActivityObservedAt = this.now();
        this.state.needsDecision = true;
      }
      if (['message.accepted', 'message.received'].includes(event.event_type) && event.payload.from !== this.self.participant_id) {
        this.state.needsDecision = true;
        // Only an attributed owner message can grant approval. Provider delivery
        // and model-generated prose never authorize the final notification step.
        if (event.payload.from === owner.participant_id && event.payload.content_type === 'application/json') {
          let value; try { value = JSON.parse(event.payload.body); } catch { continue; }
          if (value.type === 'travel.approval' && value.version === 1 && value.approved === true && value.proposal_id === this.state.proposal?.id) {
            this.state.approved = { proposal_id: value.proposal_id, source_msg_id: event.payload.msg_id };
          }
        }
      }
    }
  }

  voiceBusy() {
    const active = new Set(), speaking = new Map(), retiredAi = new Set();
    for (const event of this.state.events) {
      const { sid, speaker } = event.payload;
      if (event.event_type === 'session.invited') active.add(sid);
      if (['session.ended', 'session.failed'].includes(event.event_type)) active.delete(sid);
      if (event.event_type === 'browser.speaking') retiredAi.add(sid);
      if (event.event_type === 'session.speech') speaking.set(`${sid}:${speaker}`, { sid, speaker, active: event.payload.state === 'started' });
    }
    return [...speaking.values()].some(turn => active.has(turn.sid) && turn.active && !(turn.speaker === this.self.participant_id && retiredAi.has(turn.sid))) || (active.size && this.state.voiceActivityObservedAt != null
      && this.now() - this.state.voiceActivityObservedAt < 1500);
  }

  finishIntent(sid) {
    const call = this.state.events.find(e => e.event_type === 'session.invited' && e.payload.sid === sid);
    if (!call) return null;
    const facts = this.state.events.filter(e => e.payload.sid === sid && e.seq > call.seq);
    if (facts.some(e => ['session.ended', 'session.failed', 'session.interrupted', 'browser.speaking', 'phone.speaking'].includes(e.event_type))) return null;
    const advertised = facts.some(e => e.event_type === 'session.assistant_actions' && e.payload.source === 'vapi'
      && e.payload.participant_id === this.self.participant_id && e.payload.actions?.includes('finish_call'));
    const intent = advertised && facts.find(e => e.event_type === 'session.assistant_action' && e.payload.source === 'vapi'
      && e.payload.participant_id === this.self.participant_id && e.payload.action === 'finish_call'
      && ['completed', 'unavailable', 'participant_goodbye'].includes(e.payload.reason));
    if (intent) return intent;
    // Older voice agents have no finish tool. A reciprocal, standalone goodbye
    // is sufficient closing evidence; never infer failure from arbitrary prose.
    const goodbye = speaker => facts.find(e => e.event_type === 'session.transcript' && e.payload.is_final === true
      && e.payload.speaker === speaker && /^goodbye[.!\s]*$/i.test(e.payload.text?.trim() || ''));
    const ai = goodbye(this.self.participant_id), remote = goodbye(call.payload.participant_id);
    return ai && remote ? { seq: Math.max(ai.seq, remote.seq), payload: { sid, reason: 'participant_goodbye' } } : null;
  }

  finishReady(intent) {
    this.state.finishObservedAt ??= {};
    this.state.finishObservedAt[intent.seq] ??= this.now();
    const elapsed = this.now() - this.state.finishObservedAt[intent.seq];
    const speech = this.state.events.findLast(e => e.event_type === 'session.speech'
      && e.payload.sid === intent.payload.sid && e.payload.speaker === this.self.participant_id);
    // Remote speech must not restart the closing clock. Give the tool's goodbye
    // time to play, with a deadline if the provider loses its stopped event.
    return elapsed >= 10000 || (elapsed >= 1500 && speech?.payload.state === 'stopped'
      && this.now() - (this.state.assistantSpeechObservedAt?.[intent.payload.sid] ?? this.now()) >= 250);
  }

  requireVoiceRehearsalEvidence() {
    if (this.mode !== 'voice-only') return;
    const organizer = this.members.find(m => m.role === 'organizer');
    const owner = this.members.find(m => m.role === 'owner');
    const call = this.state.events.findLast(e => e.event_type === 'session.invited' && e.payload.participant_id === organizer?.participant_id);
    const facts = this.state.events.filter(e => e.payload.sid === call?.payload.sid);
    const browser = facts.find(e => e.event_type === 'browser.speaking' && e.payload.details?.retained_connid === call?.payload.connid);
    const failedBooker = this.state.events.some(e => e.event_type === 'session.assistant_failed'
      && this.state.events.some(invite => invite.event_type === 'session.invited' && invite.payload.sid === e.payload.sid
        && this.members.find(m => m.participant_id === invite.payload.participant_id)?.role === 'booker'));
    if (!call || failedBooker || facts.some(e => ['session.failed', 'session.assistant_failed', 'session.interrupted'].includes(e.event_type)
        || (e.event_type === 'connection.failed' && e.payload.connid === call.payload.connid))
      || !facts.some(e => e.event_type === 'session.transcript' && e.payload.speaker === organizer.participant_id && e.payload.is_final === true)
      || !browser
      || !facts.some(e => e.event_type === 'phone.speaking' && e.payload.participant_id === owner.participant_id
        && e.payload.details?.join_confirmed === true && e.payload.details?.retained_connid === call.payload.connid
        && e.payload.details?.retired_connid === browser.payload.connid)
      || !facts.some(e => e.event_type === 'session.ended'))
      throw new Error('Voice rehearsal requires an attributed organizer reply, successful owner browser and phone handoffs, and the ended confirmation call. A failed call does not confirm pickup; tell the owner what failed and wait.');
  }

  async step() {
    if (!this.state) await this.init();
    const snapshot = await this.client.snapshot(this.cid, this.state.cursor);
    this.members = snapshot.participants;
    this.capabilities = snapshot.capabilities;
    this.self = this.members.find(m => m.subject === this.client.identity);
    if (!this.self || this.self.role !== 'assistant') throw new Error('Worker requires a scoped assistant participant');
    const owner = this.members.find(m => m.role === 'owner');
    if (!owner) throw new Error('Conversation needs an owner');
    this.observe(snapshot.events);
    await this.storage.save(this.state);
    // Retired Conversations keep history and pending evidence, but never resume effects.
    this.closed = snapshot.state === 'closed';
    if (this.closed) return;
    // Drain a paged snapshot before planning from an incomplete event history.
    if (snapshot.events.length === 500) return;
    // A restarted server cannot prove a remote call has ended. Keep both
    // planning and saved effects paused until an attributed owner reconciliation.
    const interrupted = new Set();
    for (const event of this.state.events) {
      if (event.event_type === 'session.interrupted') interrupted.add(event.payload.sid);
      if (['session.ended', 'session.failed'].includes(event.event_type)) interrupted.delete(event.payload.sid);
    }
    if (interrupted.size) return;
    // After a successful human takeover the owner controls the live call. The
    // planner resumes only when that Session ends, including phone handoff.
    const humanCalls = new Set();
    for (const event of this.state.events) {
      if (['browser.speaking', 'phone.speaking'].includes(event.event_type)) humanCalls.add(event.payload.sid);
      if (['session.ended', 'session.failed'].includes(event.event_type)) humanCalls.delete(event.payload.sid);
    }
    if (humanCalls.size) return;
    // Closing intent outranks conversational turn-taking: another answering AI
    // cannot hold the line open by continually responding to the goodbye.
    if (this.state.pending?.finishIntent) {
      await this.executePending();
      return;
    }
    if (!this.state.pending) {
      const call = this.state.events.findLast(e => e.event_type === 'session.invited' && this.finishIntent(e.payload.sid));
      const intent = call && this.finishIntent(call.payload.sid);
      if (intent) {
        // An acknowledged end waits for the journal's ended event. New speech
        // must not create a fresh termination command while teardown finishes.
        if (this.state.finishedCalls?.[call.payload.sid]) return;
        const decision = { actions: [{ type: 'end_voice', sid: call.payload.sid }], source: 'voice-finish', source_seq: intent.seq };
        this.state.lastDecision = { cursor: this.state.cursor, decision };
        this.state.pending = this.prepare(decision);
        this.state.pending.finishIntent = intent;
        this.state.needsDecision = false;
        this.finishReady(intent);
        await this.storage.save(this.state);
        await this.executePending();
        return;
      }
    }
    // Wait for both people to finish speaking, including the AI readback.
    if (this.voiceBusy()) return;
    if (this.state.pending) {
      await this.executePending();
      return;
    }
    // The live voice model requests its own handoff through a typed event.
    // Do not ask a second model to infer the same intent while the caller waits.
    const organizer = this.members.find(m => m.role === 'organizer');
    const organizerCall = this.state.events.findLast(e => e.event_type === 'session.invited'
      && e.payload.participant_id === organizer?.participant_id);
    const callFacts = this.state.events.filter(e => e.payload.sid === organizerCall?.payload.sid);
    const voiceRequestsHandoff = callFacts.some(e => e.event_type === 'session.assistant_actions'
      && e.payload.participant_id === this.self.participant_id && e.payload.source === 'vapi'
      && e.payload.actions?.includes('request_browser_join'));
    const callUnavailable = callFacts.some(e => ['session.ended', 'session.failed', 'session.interrupted', 'session.assistant_failed'].includes(e.event_type)
      || (e.event_type === 'connection.failed' && e.payload.connid === organizerCall?.payload.connid));
    if (voiceRequestsHandoff && !callUnavailable) {
      const intent = callFacts.find(e => e.event_type === 'session.assistant_action'
        && e.payload.participant_id === this.self.participant_id && e.payload.source === 'vapi'
        && e.payload.action === 'request_browser_join');
      const invited = this.state.events.some(e => e.event_type === 'message.accepted' && e.payload.from === this.self.participant_id
        && e.payload.content_type === 'application/json' && (() => {
          try { const body = JSON.parse(e.payload.body); return body.type === 'travel.browser_invitation' && body.sid === organizerCall.payload.sid; }
          catch { return false; }
        })());
      if (intent && !invited && callFacts.some(e => e.event_type === 'session.transcript'
        && e.payload.speaker === organizer.participant_id && e.payload.is_final === true)) {
        const decision = { actions: [{ type: 'request_browser_join', sid: organizerCall.payload.sid }],
          source: 'voice-tool', source_seq: intent.seq };
        this.state.lastDecision = { cursor: this.state.cursor, decision };
        this.state.pending = this.prepare(decision);
        this.state.needsDecision = false;
        await this.storage.save(this.state);
        await this.executePending();
      }
      return;
    }
    if (!this.state.needsDecision) return;
    const context = {
      cid: this.cid, assistant: this.self.participant_id, demo_mode: this.mode,
      latest_owner_request: this.state.events.findLast(e => ['message.accepted', 'message.received'].includes(e.event_type)
        && e.payload.from === owner.participant_id && e.payload.content_type !== 'application/json'
        && e.payload.to?.includes(this.self.participant_id)) ?? null,
      latest_voice_failure: this.state.events.findLast(e => ['session.failed', 'session.assistant_failed', 'connection.failed', 'phone.failed'].includes(e.event_type)) ?? null,
      participants: this.members.map(({ participant_id, name, role, sms, sip }) => ({ participant_id, name, role,
        available_channels: [...(['owner', 'assistant'].includes(role) ? ['chat'] : []), ...(sms && this.mode !== 'voice-only' ? ['sms'] : []), ...(sip ? ['voice'] : [])],
      })),
      capabilities: { ...snapshot.capabilities, ...(this.mode === 'voice-only' ? { sms_configured: false, delivery: ['chat'] } : {}) },
      events: this.state.events, proposal: this.state.proposal,
      approval: this.state.approved, final_updates_accepted_for: this.state.completedProposal,
      voice_rehearsal_completed_for: this.state.completedVoiceProposal,
    };
    // A model may mistype an opaque ID. Give bounded validation feedback before
    // any effect is submitted; never replan an already pending/accepted batch.
    let batch, feedback;
    this.state.decisionRejections = [];
    for (let attempt = 1; attempt <= 3; attempt++) {
      const decision = await this.planner.decide({ ...context, ...(feedback ? { validation_feedback: feedback } : {}) });
      this.state.lastDecision = { cursor: this.state.cursor, decision };
      await this.storage.save(this.state);
      try { batch = this.prepare(decision); break; }
      catch (error) {
        feedback = { attempt, reason: error.message, previous_decision: decision };
        this.state.decisionRejections.push({ attempt, reason: error.message, provider_chat_id: decision.provider_chat_id });
        this.log({ event: 'decision.rejected', attempt, reason: error.message });
        await this.storage.save(this.state);
        if (attempt === 3) throw error;
      }
    }
    this.state.pending = batch;
    this.state.needsDecision = false;
    await this.storage.save(this.state);
    await this.executePending();
  }

  prepare(decision) {
    if (!Array.isArray(decision.actions) || decision.actions.length > 8) throw new Error('Invalid action batch');
    const seed = digest(`${this.cid}:${this.state.cursor}:${JSON.stringify(decision.actions)}`);
    const batch = { id: seed, requests: [], next: 0, results: [], attempted: [], proposal: null, completedProposal: null };
    const known = new Set(this.members.map(m => m.participant_id));
    const owner = this.members.find(m => m.role === 'owner');
    const message = (to, body, delivery = 'chat', content_type = 'text/plain') => {
      if (!Array.isArray(to) || !to.length || to.length > 32 || new Set(to).size !== to.length || to.some(id => !known.has(id))) throw new Error('Invalid recipients');
      if (!text(body) || !['chat', 'sms'].includes(delivery)) throw new Error('Invalid message');
      if (this.mode === 'voice-only' && delivery === 'sms') throw new Error('SMS is deferred in voice-only mode; no text messages may be submitted');
      if (delivery === 'sms') body = campaignSms(body);
      const suffix = `${seed}_${batch.requests.length}`;
      batch.requests.push(this.client.command('message.send', this.cid, { msg_id: `msg_${suffix}`, to, body, delivery, content_type }, { id: `env_${suffix}` }));
    };
    for (const action of decision.actions) {
      if (['call_participant', 'propose_arrangements', 'final_updates', 'complete_voice_rehearsal'].includes(action.type)) {
        const unavailable = (this.state.events || []).findLast(e => e.event_type === 'session.assistant_action'
          && e.payload.action === 'finish_call' && e.payload.reason === 'unavailable'
          && e.payload.source === 'vapi' && e.payload.participant_id === this.self.participant_id);
        const ownerRequest = (this.state.events || []).findLast(e => ['message.accepted', 'message.received'].includes(e.event_type)
          && e.payload.from === owner.participant_id && e.payload.content_type !== 'application/json');
        if (unavailable && !(ownerRequest?.seq > unavailable.seq))
          throw new Error('The answering party could not help; report missing facts and wait for a new owner request before continuing');
      }
      if (action.type === 'message') {
        message(action.to, action.body, action.delivery);
      } else if (action.type === 'propose_arrangements') {
        if (!text(action.summary, 4000) || batch.proposal || batch.completedProposal) throw new Error('Invalid proposal');
        this.requireVoiceRehearsalEvidence();
        if (this.mode === 'voice-only' && this.state.events.some(e => e.event_type === 'session.invited'
          && !this.state.events.some(end => ['session.ended', 'session.failed'].includes(end.event_type) && end.payload.sid === e.payload.sid)))
          throw new Error('Wait for the owner to end the confirmation call before proposing arrangements');
        batch.proposal = { id: `proposal_${seed}`, summary: action.summary, sandbox: true };
        message([owner.participant_id], JSON.stringify({ type: 'travel.proposal', version: 1, ...batch.proposal }), 'chat', 'application/json');
      } else if (action.type === 'final_updates') {
        if (this.mode === 'voice-only') throw new Error('SMS is deferred; use complete_voice_rehearsal after owner approval');
        if (!this.state.approved || action.proposal_id !== this.state.approved.proposal_id || action.proposal_id === this.state.completedProposal || batch.proposal || batch.completedProposal) throw new Error('Final updates need a new, explicitly approved proposal');
        const required = ['owner', 'companion', 'booker', 'organizer'].map(role => {
          const matches = this.members.filter(m => m.role === role);
          if (matches.length !== 1) throw new Error(`Exactly one ${role} required`);
          return matches[0].participant_id;
        });
        if (!Array.isArray(action.updates) || action.updates.length !== 4 || new Set(action.updates.map(u => u.to)).size !== 4 || action.updates.some(u => !required.includes(u.to) || !text(u.body, 1200))) throw new Error('Four individually addressed final updates required');
        for (const update of action.updates) message([update.to], `[Sandbox arrangements] ${update.body}`,
          this.members.find(m => m.participant_id === update.to)?.sms ? 'sms' : 'chat');
        batch.completedProposal = action.proposal_id;
      } else if (action.type === 'complete_voice_rehearsal') {
        if (this.mode !== 'voice-only' || !this.state.approved || action.proposal_id !== this.state.approved.proposal_id
          || action.proposal_id === this.state.completedVoiceProposal) throw new Error('Voice completion needs a new owner-approved proposal in voice-only mode');
        this.requireVoiceRehearsalEvidence();
        message([owner.participant_id], JSON.stringify({ type: 'travel.voice_complete', version: 1,
          proposal_id: action.proposal_id, sms: 'deferred', summary: this.state.proposal.summary }), 'chat', 'application/json');
        batch.completedVoiceProposal = action.proposal_id;
      } else if (action.type === 'request_browser_join') {
        const organizer = this.members.find(m => m.role === 'organizer');
        const call = this.state.events.find(e => e.event_type === 'session.invited'
          && e.payload.sid === action.sid && e.payload.participant_id === organizer?.participant_id);
        const facts = this.state.events.filter(e => e.payload.sid === action.sid);
        const alreadyInvited = this.state.events.some(e => {
          if (e.event_type !== 'message.accepted' || e.payload.from !== this.self.participant_id) return false;
          try { const body = JSON.parse(e.payload.body); return body.type === 'travel.browser_invitation' && body.sid === action.sid; } catch { return false; }
        });
        if (!call) {
          const sessions = this.state.events.filter(e => e.event_type === 'session.invited'
            && e.payload.participant_id === organizer?.participant_id).map(e => e.payload.sid);
          throw new Error(`Browser invitation requires a live organizer call. Copy sid exactly from its session.invited payload.sid; organizer Session IDs: ${JSON.stringify(sessions)}`);
        }
        if (!this.capabilities?.browser_handoff || !call || alreadyInvited
          || !facts.some(e => e.event_type === 'session.assistant_attached')
          || !facts.some(e => e.event_type === 'session.transcript' && e.payload.speaker === organizer.participant_id && e.payload.is_final === true)
          || facts.some(e => e.event_type === 'connection.failed' && e.payload.connid === call.payload.connid)
          || facts.some(e => ['session.ended', 'session.failed', 'session.interrupted', 'session.assistant_failed', 'browser.speaking', 'phone.speaking'].includes(e.event_type)))
          throw new Error('Browser invitation requires a live organizer call with their attributed reply, and may be sent only once for that Session');
        message([owner.participant_id], JSON.stringify({ type: 'travel.browser_invitation', version: 1,
          sid: action.sid, retained_connid: call.payload.connid }), 'chat', 'application/json');
      } else if (action.type === 'call_participant') {
        if (typeof action.to !== 'string') throw new Error('call_participant.to must be one participant ID string, not an array; only message.to is an array');
        if (!known.has(action.to) || action.to === this.self.participant_id) throw new Error('call_participant.to must exactly match a human participants[].participant_id');
        if (!this.capabilities?.assistant_voice || !this.capabilities?.operations?.includes('session.invite')) throw new Error('Assistant voice invitation is unavailable');
        if (!text(action.purpose, 4000)) throw new Error('Voice invitation needs a non-empty purpose string');
        if (this.mode === 'voice-only' && this.state.events.some(e => e.event_type === 'session.invited' && e.payload.participant_id === action.to))
          throw new Error('This participant was already called in this voice rehearsal; do not redial automatically');
        batch.requests.push(this.client.command('session.invite', this.cid, { medium: 'voice', to: action.to, purpose: action.purpose }, { id: `env_${seed}_${batch.requests.length}` }));
      } else if (action.type === 'end_voice') {
        if (!this.capabilities?.operations?.includes('session.end') || !this.state.events.some(e => e.event_type === 'session.invited' && e.payload.sid === action.sid)) throw new Error('Unknown voice Session');
        if (this.mode === 'voice-only' && this.state.events.some(e => e.event_type === 'session.invited' && e.payload.sid === action.sid
          && this.members.find(m => m.participant_id === e.payload.participant_id)?.role === 'organizer') && !this.finishIntent(action.sid))
          throw new Error('The owner ends the organizer call after browser and phone handoff; leave it open');
        batch.requests.push(this.client.command('session.end', this.cid, {}, { sid: action.sid, id: `env_${seed}_${batch.requests.length}` }));
      } else throw new Error(`Unsupported assistant action: ${action.type}`);
    }
    return batch;
  }

  async executePending() {
    const batch = this.state.pending;
    if (!batch) return;
    while (batch.next < batch.requests.length) {
      const request = batch.requests[batch.next];
      if (this.mode === 'voice-only' && request.type === 'message.send' && request.payload?.delivery === 'sms')
        throw new Error('Saved SMS work cannot execute in voice-only mode');
      if (this.mode === 'voice-only' && request.type === 'message.send' && request.payload?.content_type === 'application/json') {
        const body = JSON.parse(request.payload.body);
        if (['travel.proposal', 'travel.voice_complete'].includes(body.type)) this.requireVoiceRehearsalEvidence();
      }
      const browserInvitation = request.type === 'message.send' && request.payload?.content_type === 'application/json'
        && JSON.parse(request.payload.body).type === 'travel.browser_invitation';
      if (request.type === 'session.end' || browserInvitation) {
        // Speech can start while the planner request is in flight. Re-read the
        // journal before hanging up or ringing the owner. Keep the exact saved
        // command pending until the confirmation finishes.
        const latest = await this.client.snapshot(this.cid, this.state.cursor);
        this.observe(latest.events);
        await this.storage.save(this.state);
        if (latest.state === 'closed' || latest.events.length === 500) return;
        if (batch.finishIntent && request.type === 'session.end') {
          const intent = this.finishIntent(request.sid);
          if (!intent) {
            batch.results.push({ request_id: request.id, state: 'skipped', reason: 'call ended or handed to owner' });
            batch.next++; this.state.needsDecision = true;
            await this.storage.save(this.state);
            continue;
          }
          if (!this.finishReady(intent)) { await this.storage.save(this.state); return; }
        } else if (this.voiceBusy()) return;
        if (browserInvitation && Array.isArray(batch.attempted) && !batch.attempted.includes(request.id)) {
          const sid = JSON.parse(request.payload.body).sid;
          if (this.state.events.some(e => e.payload.sid === sid
            && (['session.ended', 'session.failed', 'session.interrupted', 'session.assistant_failed', 'browser.speaking', 'phone.speaking'].includes(e.event_type)
              || (e.event_type === 'connection.failed' && e.payload.connid === JSON.parse(request.payload.body).retained_connid)))) {
            batch.results.push({ request_id: request.id, state: 'skipped', reason: 'organizer call no longer available' });
            batch.next++; this.state.needsDecision = true;
            await this.storage.save(this.state);
            this.log({ event: 'action.skipped', request_id: request.id, reason: 'organizer call no longer available' });
            continue;
          }
        }
      }
      // Timeout/disconnect leaves the exact request pending. Reconnect replays
      // it; the server's durable idempotency record prevents duplicate sends.
      if (Array.isArray(batch.attempted) && !batch.attempted.includes(request.id)) {
        batch.attempted.push(request.id);
        await this.storage.save(this.state);
      }
      const response = await this.client.request(request);
      if (batch.finishIntent && request.type === 'session.end') {
        this.state.finishedCalls ??= {};
        this.state.finishedCalls[request.sid] = { request_id: request.id, source_seq: batch.finishIntent.seq };
      }
      batch.results.push({ request_id: request.id, response }); batch.next++;
      await this.storage.save(this.state);
      this.log({ event: 'action.accepted', cid: this.cid, request_id: request.id });
    }
    if (batch.proposal) { this.state.proposal = batch.proposal; this.state.approved = null; }
    if (batch.completedProposal) this.state.completedProposal = batch.completedProposal;
    if (batch.completedVoiceProposal) this.state.completedVoiceProposal = batch.completedVoiceProposal;
    this.state.lastBatch = batch;
    this.state.pending = null;
    await this.storage.save(this.state);
  }
}

export async function runWorkerLoop({ client, worker, stopped = () => false,
  pause = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds)),
  log = value => console.error(JSON.stringify(value)) }) {
  let planningFailures = 0, transportFailures = 0;
  while (!stopped()) {
    let connecting = false;
    try {
      if (!client.authenticated) { connecting = true; await client.connect(); connecting = false; }
      await worker.step();
      planningFailures = 0; transportFailures = 0;
      if (worker.closed) return;
    } catch (error) {
      if (stopped()) return;
      const transport = connecting || (error instanceof UctpError && !error.code
        && /timed out|disconnected|not connected/.test(error.message));
      const retry = transport ? ++transportFailures <= 3 : error.retryable && ++planningFailures <= 3;
      log({ event: 'worker.error', message: error.message, code: error.code,
        transport_reason: error.transportReason, elapsed_ms: error.elapsedMs,
        operation: error.request?.type, request_id: error.request?.id,
        retry: !!retry, attempt: transport ? transportFailures : planningFailures });
      if (!retry) throw error;
      // Each mutating command was durably saved before send. Reconnection
      // replays that exact envelope; read snapshots can safely be requested anew.
      if (transport) client.close();
    }
    if (!stopped()) await pause(1000);
  }
}

async function main() {
  const cid = process.env.CONFERENCE_CID;
  const token = process.env.CONFERENCE_ASSISTANT_TOKEN;
  if (!cid || !token) throw new Error('CONFERENCE_CID and CONFERENCE_ASSISTANT_TOKEN required');
  const statePath = process.env.CONFERENCE_WORKER_STATE || `var/conference/${cid}/worker.json`;
  const planner = new VapiPlanner({ apiKey: process.env.VAPI_PRIVATE_KEY, model: process.env.VAPI_CHAT_MODEL || undefined });
  const client = new UctpClient(process.env.UCTP_URL || 'ws://127.0.0.1:7443', token);
  await mkdir(dirname(statePath), { recursive: true, mode: 0o700 });
  const lock = await open(`${statePath}.lock`, 'wx', 0o600).catch(() => { throw new Error('Worker lock exists; verify the previous worker stopped before removing it'); });
  await lock.writeFile(String(process.pid));
  const worker = new ConferenceWorker({ client, cid, planner, mode: process.env.CONFERENCE_DEMO_MODE || 'full', storage: new FileState(statePath), log: value => console.log(JSON.stringify(value)) });
  let stopped = false;
  const stop = () => { stopped = true; client.close(); };
  process.on('SIGINT', stop); process.on('SIGTERM', stop);
  try {
    await runWorkerLoop({ client, worker, stopped: () => stopped });
  } finally { client.close(); await lock.close(); await unlink(`${statePath}.lock`); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
