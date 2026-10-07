// A presentation of journal facts, never a source of communication outcomes.
export class ConversationProjection {
  constructor(cid, participants) {
    this.cid = cid; this.members = new Map(participants.map(p => [p.participant_id, p]));
    this.owner = participants.find(p => p.role === 'owner')?.participant_id;
    this.assistant = participants.find(p => p.role === 'assistant')?.participant_id;
    this.seq = 0; this.sessions = new Map(); this.messages = new Map(); this.deliveries = new Map();
    this.facts = []; this.latest = {}; this.task = null; this.proposal = null; this.approval = null; this.voiceComplete = null;
  }
  apply(event) {
    if ((event.cid && event.cid !== this.cid) || event.seq <= this.seq) return false;
    this.seq = event.seq;
    const p = event.payload; const type = event.event_type;
    if (type === 'message.accepted') {
      this.messages.set(p.msg_id, { ...p, seq: event.seq });
      if (p.from === this.assistant) this.latest.worker = event;
      if (p.delivery === 'sms') this.latest.sms = event;
      if (!this.task && p.from === this.owner && (p.to || []).includes(this.assistant) && p.content_type !== 'application/json') this.task = event;
      if (p.content_type === 'application/json') {
        try {
          const body = JSON.parse(p.body);
          if (p.from === this.assistant && body.type === 'travel.proposal') { this.proposal = { ...body, event }; this.approval = null; }
          if (p.from === this.assistant && p.delivery === 'chat' && (p.to || []).includes(this.owner)
            && body.type === 'travel.browser_invitation' && body.version === 1) {
            const call = this.sessions.get(body.sid);
            if (call && this.members.get(call.participant)?.role === 'organizer' && body.retained_connid === call.remote)
              call.invitation = { ...body, event };
          }
          if (p.from === this.owner && body.type === 'travel.approval' && body.approved === true && body.version === 1 && body.proposal_id === this.proposal?.id && !this.approval) this.approval = event;
          if (p.from === this.assistant && body.type === 'travel.voice_complete' && body.version === 1 && body.sms === 'deferred'
            && body.proposal_id === this.proposal?.id && this.approval && (p.to || []).includes(this.owner)) this.voiceComplete = event;
        } catch {}
      }
      for (const d of p.deliveries || []) this.deliveries.set(d.id, { ...d, msg_id: p.msg_id, state: d.state, seq: event.seq });
    }
    if (type === 'message.delivery') {
      this.deliveries.set(p.delivery_id, { ...this.deliveries.get(p.delivery_id), ...p, seq: event.seq });
      this.latest.sms = event;
    }
    if (type === 'message.received') { this.latest.sms = event; this.facts.push(event); }
    if (type === 'session.invited') {
      this.sessions.set(p.sid, { sid: p.sid, remote: p.connid, participant: p.participant_id, state: 'accepted', sip: 'accepted', ai: 'idle', browser: 'idle', event });
      this.latest.sip = event; this.latest.vapi = null; this.latest.browser = null; this.latest.phone = null;
      if (p.initiated_by === this.assistant) this.latest.worker = event;
    }
    const session = this.sessions.get(p.sid);
    if (session) {
      if (type.startsWith('connection.') && p.connid === session.remote) {
        if (p.state !== 'progress') session.sip = p.state;
        this.latest.sip = event; session.event = event;
      }
      if (type === 'session.assistant_attached' && !['ended', 'failed', 'interrupted'].includes(session.state)) { session.ai = 'active'; session.aiConnection = p.connid; session.bridge = p.bridge_id; this.latest.vapi = event; session.state = 'active'; }
      if (type === 'session.assistant_failed') { session.ai = 'failed'; this.latest.vapi = event; }
      if (type === 'connection.offered' && p.transport === 'webrtc') { session.browser = 'offered'; session.browserConnection = p.connid; this.latest.browser = event; }
      if (type.startsWith('browser.')) {
        session.browser = p.state; session.browserConnection = p.connid; this.latest.browser = event;
        if (p.state === 'speaking') {
          // A handoff is accepted visually only when the retained ID matches
          // the original invitation. A new connection is not a retained call.
          session.retained = p.details?.retained_connid === session.remote;
          if (!session.retained) session.browser = 'unknown';
          if (session.retained) { session.ai = 'retired'; session.bridge = p.details.bridge_id; this.latest.vapi = event; }
        }
      }
      if (type.startsWith('phone.') && !['ended', 'failed', 'interrupted'].includes(session.state)) {
        session.phone = p.state; session.phoneConnection = p.connid; this.latest.phone = event;
        if (p.state === 'speaking') {
          const verified = p.details?.retained_connid === session.remote
            && p.details?.retired_connid === session.browserConnection && p.details?.join_confirmed === true;
          session.phone = verified ? 'speaking' : 'unknown';
          if (verified) { session.browser = 'retired'; session.bridge = p.details.bridge_id; session.retained = true; this.latest.browser = event; }
        }
      }
      if (type === 'session.interrupted') { this.latest.sip = event; this.latest.vapi = event; this.latest.browser = event; session.state = 'interrupted'; session.sip = 'unknown'; session.ai = 'unknown'; session.browser = 'unknown'; }
      if (['session.ended', 'session.failed'].includes(type)) { this.latest.sip = event; this.latest.vapi = event; this.latest.browser = event; session.state = p.state || type.split('.')[1]; session.sip = session.state; session.ai = 'ended'; session.browser = 'ended'; session.event = event; }
      if (type === 'session.transcript' && p.is_final === true) this.facts.push(event);
    }
    return true;
  }
  get voice() { return [...this.sessions.values()].at(-1); }
  get browserInvitation() {
    const call = this.voice;
    return call?.state === 'active' && call.sip === 'connected' && call.ai === 'active'
      && ['idle', 'ended'].includes(call.browser) && call.invitation ? call.invitation : null;
  }
  name(pid) { return this.members.get(pid)?.name || 'Participant'; }
  finalUpdates() {
    const result = [];
    if (!this.approval) return result;
    for (const member of this.members.values()) {
      if (!['owner', 'companion', 'booker', 'organizer'].includes(member.role)) continue;
      const deliveries = [...this.deliveries.values()].filter(d => {
        const message = this.messages.get(d.msg_id);
        return d.participant_id === member.participant_id && message?.from === this.assistant
          && message.seq > this.approval.seq && message.body?.startsWith('[Sandbox arrangements]');
      });
      result.push({ member, delivery: deliveries.at(-1) });
    }
    return result;
  }
  routeFor(event) {
    const type = event.event_type; const p = event.payload;
    if (type.startsWith('message.')) return p.delivery === 'sms' || type === 'message.delivery' || type === 'message.received' ? 'sms' : 'control';
    if (type.startsWith('browser.') || (type === 'connection.offered' && p.transport === 'webrtc')) return 'browser';
    if (type.startsWith('phone.')) return 'phone';
    if (type.startsWith('session.assistant') || type.startsWith('session.transcript')) return 'vapi';
    if (p.sid) return 'sip';
    return 'control';
  }
  network(connected, smsMode) {
    const v = this.voice;
    const finished = v && ['ended', 'failed', 'interrupted'].includes(v.state);
    const terminal = v?.state === 'interrupted' ? 'unknown' : 'ended';
    const sip = finished ? terminal : v?.sip || 'idle';
    const browser = finished ? terminal : v?.browser || 'idle';
    const ai = finished ? terminal : v?.ai || 'idle';
    const phone = finished ? terminal : v?.phone || 'idle';
    const media = finished ? terminal : (ai === 'active' || (browser === 'speaking' && v?.retained) || (phone === 'speaking' && v?.retained)) ? 'active' : 'idle';
    const sms = this.latest.sms;
    const smsState = sms ? (sms.event_type === 'message.received' ? 'replied' : sms.payload.state || 'accepted') : smsMode === 'deferred' ? 'deferred' : 'idle';
    return {
      assistantName: this.members.get(this.assistant)?.name || 'AI assistant',
      target: v ? this.name(v.participant) : 'Participant endpoint',
      sid: v?.sid, remote: v?.remote, retained: v?.retained === true,
      smsProvider: smsMode === 'deferred' ? 'SMS awaiting campaign approval' : smsMode === 'fake' ? 'SMS fixture' : smsMode === 'telnyx' ? 'Telnyx Messaging' : 'SMS connector',
      smsRecipients: new Set([...this.messages.values()].filter(m => m.delivery === 'sms').flatMap(m => m.to || [])).size,
      edges: [
        { id: 'worker', route: 'control', kind: 'control', state: this.latest.worker ? 'observed' : 'idle', label: this.latest.worker ? 'UCTP · action observed' : 'UCTP · awaiting assistant', seq: this.latest.worker?.seq },
        { id: 'owner', route: 'control', kind: 'control', state: connected === undefined ? 'historical' : connected ? 'connected' : 'offline', label: connected === undefined ? 'UCTP · client control' : `UCTP · ${connected ? 'connected' : 'offline'}` },
        { id: 'browser', route: 'browser', kind: 'media', state: browser, label: `WebRTC · ${browser === 'speaking' && v?.retained ? 'bridge active' : browser === 'answered' ? 'media ready' : browser}`, seq: this.latest.browser?.seq },
        { id: 'phone', route: 'phone', kind: 'media', state: phone, label: `SIP/PSTN · ${phone === 'speaking' ? 'bridge active' : phone === 'answered' ? 'press 1 to join' : phone}`, seq: this.latest.phone?.seq },
        { id: 'sip', route: 'sip', kind: 'control', state: sip, label: `SIP · ${sip}`, seq: this.latest.sip?.seq },
        { id: 'rtp', route: 'sip', kind: 'media', state: media, label: `RTP · ${media === 'active' ? 'bridge active' : media}`, seq: (this.latest.browser || this.latest.vapi)?.seq },
        { id: 'vapi', route: 'vapi', kind: 'media', state: ai, label: `Audio WebSocket · ${ai}`, seq: this.latest.vapi?.seq },
        { id: 'sms', route: 'sms', kind: 'message', state: smsState, label: `SMS · ${smsState}`, seq: sms?.seq },
      ],
    };
  }
}

// The stage projection hides telephone numbers in bodies and evidence. The
// authenticated command and stored event remain unchanged.
export function stageText(value) {
  return String(value).replace(/\+\d[\d ()-]{7,24}\d/g, '[phone number]');
}
