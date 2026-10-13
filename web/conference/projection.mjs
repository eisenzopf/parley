// A presentation of journal facts, never a source of communication outcomes.
export class ConversationProjection {
  constructor(cid, participants) {
    this.cid = cid; this.members = new Map(participants.map(p => [p.participant_id, p]));
    this.owner = participants.find(p => p.role === 'owner')?.participant_id;
    this.assistant = participants.find(p => p.role === 'assistant')?.participant_id;
    this.seq = 0; this.sessions = new Map(); this.messages = new Map(); this.deliveries = new Map();
    this.facts = []; this.latest = {}; this.task = null; this.proposal = null; this.approval = null; this.voiceComplete = null;
    this.milestones = [];
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
      this.sessions.set(p.sid, { sid: p.sid, remote: p.connid, participant: p.participant_id, transport: p.transport || 'sip', state: 'accepted', sip: 'accepted', ai: 'idle', browser: 'idle', event });
      this.latest.sip = event; this.latest.vapi = null; this.latest.browser = null; this.latest.phone = null;
      if (p.initiated_by === this.assistant) this.latest.worker = event;
    }
    const session = this.sessions.get(p.sid);
    if (session) {
      if (type === 'session.speech') {
        if (p.state === 'started') session.speaker = p.speaker;
        else if (session.speaker === p.speaker) session.speaker = null;
      }
      if (type.startsWith('connection.') && p.connid === session.remote) {
        if (p.state !== 'progress') session.sip = p.state;
        this.latest.sip = event; session.event = event;
      }
      if (type === 'session.assistant_attached' && !['ended', 'failed', 'interrupted'].includes(session.state)) { session.ai = 'active'; session.aiConnection = p.connid; session.bridge = p.bridge_id; this.latest.vapi = event; session.state = 'active'; }
      if (type === 'session.assistant_failed') { session.ai = 'failed'; this.latest.vapi = event; }
      if (type === 'session.organizer_held' && p.connid === session.remote) { session.held = true; session.ai = 'held'; this.latest.vapi = event; }
      if (type === 'connection.offered' && p.transport === 'webrtc') { session.browser = 'offered'; session.browserConnection = p.connid; this.latest.browser = event; }
      if (type.startsWith('browser.')) {
        session.browser = p.state; session.browserConnection = p.connid; this.latest.browser = event;
        if (p.state === 'consulting') { session.ai = 'active'; this.latest.vapi = event; }
        if (p.state === 'speaking') {
          // A handoff is accepted visually only when the retained ID matches
          // the original invitation. A new connection is not a retained call.
          session.retained = p.details?.retained_connid === session.remote;
          if (!session.retained) session.browser = 'unknown';
          if (session.retained) { session.held = false; session.ai = 'retired'; session.bridge = p.details.bridge_id; this.latest.vapi = event; }
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
    const scene = this.scene();
    const key = `${this.voice?.sid || ''}:${scene.phase}`;
    if (this.milestones.at(-1)?.key !== key) this.milestones.push({ key, seq: event.seq, label: scene.title, time: event.created_at });
    return true;
  }
  get voice() { return [...this.sessions.values()].at(-1); }
  get browserInvitation() {
    const call = this.voice;
    return call?.state === 'active' && call.sip === 'connected' && ['active', 'held'].includes(call.ai)
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
          && message.seq > this.approval.seq && /^(Rudeless Thelve: )?\[Sandbox arrangements\]/.test(message.body || '');
      });
      const chat = [...this.messages.values()].filter(message => message.delivery === 'chat'
        && message.to?.includes(member.participant_id) && message.from === this.assistant
        && message.seq > this.approval.seq && message.body?.startsWith('[Sandbox arrangements]')).at(-1);
      result.push({ member, delivery: deliveries.at(-1), chat });
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
  scene() {
    const v = this.voice, finished = v && ['ended', 'failed', 'interrupted'].includes(v.state);
    const roles = ['owner', 'assistant', 'booker', 'organizer'];
    const defaults = { owner: 'Jonathan', assistant: 'David', booker: 'Reservationist', organizer: 'Jeff' };
    const cards = roles.map(role => {
      const member = [...this.members.values()].find(m => m.role === role);
      const previous = [...this.sessions.values()].findLast(s => s.participant === member?.participant_id && s !== v && ['ended', 'failed', 'interrupted'].includes(s.state));
      return { id: role, participant: member?.participant_id, name: member?.name || defaults[role], role,
        device: role === 'owner' ? 'Browser' : role === 'assistant' ? 'AI voice' : 'Phone',
        status: previous ? role === 'owner' ? 'Waiting for David' : 'Call ended' : 'Ready', state: previous && role !== 'owner' ? 'ended' : 'idle', speaking: !!v?.speaker && !finished && v.speaker === member?.participant_id, seq: previous?.event.seq };
    });
    const card = role => cards.find(c => c.id === role);
    const set = (role, status, state = 'active', seq = v?.event?.seq) => Object.assign(card(role), { status, state, seq });
    const links = [];
    let phase = 'ready', title = 'Ready when you are', description = 'Call David to start. Each person stays in one place as the conversation moves.';
    const stage = (key, heading, detail) => { phase = key; title = heading; description = detail; };
    const link = (from, to, label, state = 'active', kind = 'audio', seq = v?.event?.seq, route = 'sip') => links.push({ id: `${from}-${to}`, from, to, label, state, kind, seq, route });
    const target = this.members.get(v?.participant)?.role;
    const sms = this.latest.sms;
    const organizer = card('organizer').participant;
    const smsMessage = sms && (sms.event_type === 'message.accepted' ? sms.payload : this.messages.get(sms.payload.msg_id || this.deliveries.get(sms.payload.delivery_id)?.msg_id));
    const toJeff = sms && ((sms.event_type === 'message.received' && sms.payload.from === organizer) || smsMessage?.to?.includes(organizer));
    if (v && !finished) {
      if (target === 'owner' && v.transport === 'webrtc') {
        stage(v.ai === 'active' ? 'opening' : 'opening-dialing', v.ai === 'active' ? 'Jonathan talks to David' : 'Connecting to David', 'Jonathan tells David he missed his flight.');
        set('owner', v.ai === 'active' ? 'Talking to David' : 'Connecting…'); set('assistant', 'With Jonathan');
        link('owner', 'assistant', 'WebRTC', v.ai === 'active' ? 'active' : 'dialing', 'audio', this.latest.vapi?.seq || v.event.seq, 'browser');
      } else if (target === 'booker') {
        stage(v.ai === 'active' ? 'booking' : 'booking-dialing', v.ai === 'active' ? 'David arranges the flight' : 'Calling the reservationist', 'Airline, flight number, departure and arrival. Then a quick goodbye.');
        set('assistant', 'Booking the flight'); set('booker', v.ai === 'active' ? 'Talking to David' : 'Phone ringing…', v.ai === 'active' ? 'active' : 'ringing');
        link('assistant', 'booker', 'Phone', v.ai === 'active' ? 'active' : 'dialing', 'audio', this.latest.vapi?.seq || v.event.seq);
      } else if (target === 'organizer') {
        if (v.browser === 'speaking' && v.retained || v.phone === 'speaking' && v.retained) {
          const mobile = v.phone === 'speaking';
          stage(mobile ? 'mobile' : 'connected', mobile ? 'Jonathan takes it on mobile' : 'Jonathan and Jeff are connected', 'Jeff stays on the same call. David has handed the conversation over.');
          set('assistant', 'Handoff complete', 'retired', this.latest.browser?.seq); set('owner', 'Talking to Jeff'); set('organizer', 'Talking to Jonathan');
          card('owner').device = mobile ? 'Mobile' : 'Browser';
          link('owner', 'organizer', mobile ? 'Phone ↔ Phone' : 'WebRTC ↔ Phone', 'active', 'audio', (mobile ? this.latest.phone : this.latest.browser)?.seq, mobile ? 'phone' : 'browser');
        } else if (v.browser === 'consulting') {
          stage('private', 'David checks with Jonathan', 'A private conversation with David. Jeff is on hold and cannot hear it.');
          set('assistant', 'With Jonathan'); set('owner', 'Private call'); set('organizer', 'On hold', 'held', this.latest.browser?.seq);
          link('owner', 'assistant', 'WebRTC · private', 'active', 'audio', this.latest.browser?.seq, 'browser');
          link('assistant', 'organizer', 'On hold', 'held', 'audio', this.latest.browser?.seq);
        } else {
          const ringing = v.held || !!this.browserInvitation || ['offered', 'answered'].includes(v.browser);
          stage(ringing ? 'ringing' : v.ai === 'active' ? 'organizer' : 'organizer-dialing', ringing ? 'David is calling Jonathan' : v.ai === 'active' ? 'David updates Jeff' : 'Calling Jeff', ringing ? 'Jonathan’s browser rings. Jeff’s call stays connected while Jonathan answers.' : 'David shares the new itinerary and lets Jeff respond.');
          set('assistant', ringing ? 'Calling Jonathan' : v.ai === 'active' ? 'With Jeff' : 'Calling Jeff'); set('organizer', v.held ? 'On hold' : v.ai === 'active' ? 'Talking to David' : 'Phone ringing…', v.held ? 'held' : v.ai === 'active' ? 'active' : 'ringing');
          link('assistant', 'organizer', v.held ? 'On hold' : 'Phone', v.held ? 'held' : v.ai === 'active' ? 'active' : 'dialing', 'audio', this.latest.vapi?.seq);
          if (ringing) { set('owner', 'Browser ringing…', 'ringing', this.latest.browser?.seq || v.invitation?.event.seq); link('owner', 'assistant', 'WebRTC · ringing', 'dialing', 'audio', card('owner').seq, 'browser'); }
        }
        if (['prepared', 'dialing', 'answered', 'confirmed', 'committing'].includes(v.phone)) {
          const answered = ['answered', 'confirmed', 'committing'].includes(v.phone);
          const confirmed = ['confirmed', 'committing'].includes(v.phone);
          stage(confirmed ? 'moving-confirmed' : answered ? 'moving-answer' : 'moving-dialing', confirmed ? 'Switching to mobile' : answered ? 'Switching to mobile' : 'Calling Jonathan’s mobile', 'The browser stays connected until Jonathan answers and the mobile audio is connected.');
          card('owner').pending = confirmed ? 'Mobile confirmed · connecting…' : answered ? 'Mobile answered · connecting…' : 'Mobile ringing…';
          card('owner').pendingSeq = this.latest.phone?.seq;
        } else if (v.phone === 'failed' && v.browser === 'speaking') card('owner').pending = 'Mobile failed · browser retained';
      }
      if (v.ai === 'failed' || v.state === 'interrupted' || v.browser === 'unknown' || v.phone === 'unknown') {
        stage('attention', 'Connection needs attention', 'Open the connection details to see the recorded status.');
      }
    } else if (v && finished) {
      set('assistant', 'Call ended', 'ended');
      if (target) set(target, 'Call ended', 'ended');
      if (v.retained) { set('owner', 'Call ended', 'ended'); card('owner').device = v.phone === 'speaking' ? 'Mobile' : 'Browser'; set('assistant', 'Handoff complete', 'retired'); }
      stage(v.state === 'ended' ? 'ended' : 'attention', target === 'booker' ? 'Reservationist call ended' : 'Call ended', target === 'booker' ? 'David can now coordinate the update with Jeff.' : 'The conversation history and connections are available to replay.');
    }
    if (toJeff && (!v || finished && sms.seq > v.event.seq)) {
      stage(sms.event_type === 'message.received' ? 'sms-reply' : 'sms', sms.event_type === 'message.received' ? 'Jeff replied by text' : 'David texts Jeff', 'David asks “Can I call you?” and waits for Jeff’s reply before calling.');
      set('assistant', 'Coordinating by SMS', 'message', sms.seq); set('organizer', sms.event_type === 'message.received' ? 'SMS received' : 'Flight update by SMS', 'message', sms.seq);
      link('assistant', 'organizer', sms.event_type === 'message.received' ? 'SMS · reply received' : `SMS · ${sms.payload.state || 'accepted'}`, 'message', 'message', sms.seq, 'sms');
    }
    return { cards, links, phase, title, description };
  }
  network(connected, smsMode) {
    const v = this.voice;
    const finished = v && ['ended', 'failed', 'interrupted'].includes(v.state);
    const terminal = v?.state === 'interrupted' ? 'unknown' : 'ended';
    const opening = v?.transport === 'webrtc';
    const sip = opening ? 'idle' : finished ? terminal : v?.sip || 'idle';
    const browser = finished ? terminal : opening && v?.ai === 'active' ? 'active' : v?.browser || 'idle';
    const ai = finished ? terminal : v?.ai || 'idle';
    const phone = finished ? terminal : v?.phone || 'idle';
    const media = opening ? 'idle' : finished ? terminal : browser === 'consulting' || v?.held ? 'held' : (ai === 'active' || (browser === 'speaking' && v?.retained) || (phone === 'speaking' && v?.retained)) ? 'active' : 'idle';
    const sms = this.latest.sms;
    const smsState = sms ? (sms.event_type === 'message.received' ? 'replied' : sms.payload.state || 'accepted') : smsMode === 'deferred' ? 'deferred' : 'idle';
    return {
      scene: this.scene(),
      assistantName: this.members.get(this.assistant)?.name || 'AI assistant',
      target: v ? this.name(v.participant) : 'Participant endpoint',
      sid: v?.sid, remote: v?.remote, retained: v?.retained === true,
      smsProvider: smsMode === 'deferred' ? 'SMS awaiting campaign approval' : smsMode === 'fake' ? 'SMS fixture' : smsMode === 'telnyx' ? 'Telnyx Messaging' : 'SMS connector',
      smsRecipients: new Set([...this.messages.values()].filter(m => m.delivery === 'sms').flatMap(m => m.to || [])).size,
      edges: [
        { id: 'worker', route: 'control', kind: 'control', state: this.latest.worker ? 'observed' : 'idle', label: this.latest.worker ? 'UCTP · action observed' : 'UCTP · awaiting assistant', seq: this.latest.worker?.seq },
        { id: 'owner', route: 'control', kind: 'control', state: connected === undefined ? 'historical' : connected ? 'connected' : 'offline', label: connected === undefined ? 'UCTP · client control' : `UCTP · ${connected ? 'connected' : 'offline'}` },
        { id: 'browser', route: 'browser', kind: 'media', state: browser, label: `WebRTC · ${browser === 'speaking' && v?.retained ? 'bridge active' : browser === 'consulting' ? 'private call with David' : browser === 'answered' ? 'media ready' : browser}`, seq: this.latest.browser?.seq },
        { id: 'phone', route: 'phone', kind: 'media', state: phone, label: `SIP/PSTN · ${phone === 'speaking' ? 'bridge active' : phone === 'answered' ? 'connecting automatically' : phone}`, seq: this.latest.phone?.seq },
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
