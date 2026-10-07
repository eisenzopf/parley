import test from 'node:test';
import assert from 'node:assert/strict';
import { ConferenceWorker, runWorkerLoop } from './worker.mjs';
import { UctpError } from '../../clients/uctp-js/client.mjs';

const confirmedVoice = () => [
  { event_type: 'session.invited', payload: { sid: 'ses_confirm', participant_id: 'organizer', connid: 'conn_organizer' } },
  { event_type: 'session.transcript', payload: { sid: 'ses_confirm', speaker: 'organizer', is_final: true, text: 'Pickup is confirmed.' } },
  { event_type: 'browser.speaking', payload: { sid: 'ses_confirm', connid: 'conn_browser', details: { retained_connid: 'conn_organizer' } } },
  { event_type: 'phone.speaking', payload: { sid: 'ses_confirm', participant_id: 'owner', details: { retained_connid: 'conn_organizer', retired_connid: 'conn_browser', join_confirmed: true } } },
  { event_type: 'session.ended', payload: { sid: 'ses_confirm' } },
];

test('voice planning waits for stopped speech and a quiet interval across restart', async () => {
  let now = 0, plans = 0;
  const events = [
    { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_booker' } },
    { seq: 2, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'booker', state: 'started' } },
    { seq: 3, event_type: 'session.transcript', payload: { sid: 'ses_booker', speaker: 'booker', text: 'There are two seats...' } },
  ];
  let saved = { cursor: 0, events: [], pending: null };
  const options = { cid: 'conv_speech', now: () => now,
    client: { identity: 'assistant', snapshot: async (_cid, cursor) => ({ state: 'open', capabilities: {}, events: events.filter(e => e.seq > cursor), participants: [
      { participant_id: 'owner', role: 'owner' }, { participant_id: 'booker', role: 'booker' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ] }) },
    storage: { load: async () => structuredClone(saved), save: async state => { saved = structuredClone(state); } },
    planner: { decide: async () => { plans++; return { actions: [] }; } },
  };
  let worker = new ConferenceWorker(options);
  await worker.step(); assert.equal(plans, 0);
  now = 5000; await worker.step(); assert.equal(plans, 0, 'ongoing speech blocks planning even after the quiet timer');
  events.push({ seq: 4, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'booker', state: 'stopped' } });
  await worker.step(); assert.equal(plans, 0);
  worker = new ConferenceWorker(options);
  now = 6499; await worker.step(); assert.equal(plans, 0, 'restart preserves the quiet interval');
  now = 6500; await worker.step(); assert.equal(plans, 1);
  events.push({ seq: 5, event_type: 'session.transcript', payload: { sid: 'ses_booker', speaker: 'booker', text: 'And the terminal is C.' } });
  now = 6600; await worker.step(); assert.equal(plans, 1);
  now = 8100; await worker.step(); assert.equal(plans, 2, 'a late final fragment restarts the quiet interval');
});

test('planner sees participant channels without receiving private routing endpoints', async () => {
  let context;
  const worker = new ConferenceWorker({ cid: 'conv_channels',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'open', capabilities: {}, events: [
      { seq: 1, event_type: 'message.accepted', payload: { from: 'owner', body: 'Coordinate the trip' } },
    ], participants: [
      { participant_id: 'owner', role: 'owner', sms: '+14155550101' },
      { participant_id: 'booker', role: 'booker', sms: '+14155550103', sip: 'sip:private-route@example.invalid' },
      { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ] }) },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async () => {} },
    planner: { decide: async value => { context = value; return { actions: [] }; } },
  });
  await worker.step();
  assert.deepEqual(context.participants.map(p => p.available_channels), [['chat', 'sms'], ['sms', 'voice'], ['chat']]);
  assert.ok(!JSON.stringify(context).includes('+1415555'));
  assert.ok(!JSON.stringify(context).includes('private-route'));
});

test('rejected provider decisions remain diagnosable without submitting effects', async () => {
  let saved, attempts = 0;
  const decision = { provider_chat_id: 'fixture-rejected', actions: [
    { type: 'message', to: ['unknown'], body: 'Invalid route', delivery: 'sms' },
  ] };
  const worker = new ConferenceWorker({ cid: 'conv_rejected',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'open', capabilities: {}, events: [
      { seq: 1, event_type: 'message.accepted', payload: { from: 'owner', body: 'Coordinate the trip' } },
    ], participants: [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' }] }),
    request: async () => assert.fail('invalid decision must not submit effects') },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async s => { saved = structuredClone(s); } },
    planner: { decide: async () => { attempts++; return decision; } },
  });
  await assert.rejects(worker.step(), /Invalid recipients/);
  assert.deepEqual(saved.lastDecision, { cursor: 1, decision });
  assert.equal(saved.pending, null);
  assert.equal(saved.needsDecision, true);
  assert.equal(attempts, 3);
  assert.equal(saved.decisionRejections.length, 3);
});

test('validation feedback repairs an invalid route before exactly one submission', async () => {
  let attempts = 0, saved; const sent = [];
  const worker = new ConferenceWorker({ cid: 'conv_repair',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'open', capabilities: {}, events: [
      { seq: 1, event_type: 'message.accepted', payload: { from: 'par_owner', body: 'Help coordinate' } },
    ], participants: [{ participant_id: 'par_owner', role: 'owner' }, { participant_id: 'par_ai', role: 'assistant', subject: 'assistant' }] }),
    command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }),
    request: async command => { sent.push(command); return { type: 'ack' }; } },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async state => { saved = structuredClone(state); } },
    planner: { decide: async context => {
      attempts++;
      if (attempts === 1) return { actions: [{ type: 'message', to: ['owner'], delivery: 'chat', body: 'I can help.' }] };
      assert.equal(context.validation_feedback.reason, 'Invalid recipients');
      assert.equal(sent.length, 0);
      return { actions: [{ type: 'message', to: ['par_owner'], delivery: 'chat', body: 'I can help.' }] };
    } },
  });
  await worker.step();
  assert.equal(attempts, 2); assert.equal(sent.length, 1);
  assert.deepEqual(sent[0].payload.to, ['par_owner']); assert.equal(saved.decisionRejections.length, 1);
  await worker.step(); assert.equal(attempts, 2); assert.equal(sent.length, 1);
});

test('restart pauses saved effects and planning until the call is reconciled', async () => {
  const members = [
    { participant_id: 'owner', subject: 'owner', role: 'owner' },
    { participant_id: 'assistant', subject: 'assistant', role: 'assistant' },
  ];
  const interrupted = { seq: 1, event_type: 'session.interrupted', payload: { sid: 'ses_call' } };
  const ended = { seq: 2, event_type: 'session.ended', payload: { sid: 'ses_call', source: 'owner_verification' } };
  let reconciled = false, plans = 0, sent = [];
  let saved = { version: 1, cid: 'conv_restart', cursor: 0, events: [], needsDecision: false,
    pending: { requests: [{ id: 'env_saved' }], next: 0, results: [] } };
  const storage = { load: async () => structuredClone(saved), save: async s => { saved = structuredClone(s); } };
  const client = { identity: 'assistant',
    snapshot: async (cid, cursor) => ({ participants: members, capabilities: {}, events: [interrupted, ...(reconciled ? [ended] : [])].filter(e => e.seq > cursor) }),
    request: async frame => { sent.push(frame.id); return { id: 'ack_saved' }; },
  };
  const planner = { decide: async () => { plans++; return { actions: [] }; } };
  let worker = new ConferenceWorker({ client, cid: 'conv_restart', planner, storage });
  await worker.step();
  assert.deepEqual(sent, []); assert.equal(plans, 0); assert.equal(saved.cursor, 1);
  assert.ok(saved.pending, 'keep exact saved effects for later reconciliation');
  worker = new ConferenceWorker({ client, cid: 'conv_restart', planner, storage });
  await worker.step(); assert.deepEqual(sent, []); assert.equal(plans, 0);
  reconciled = true;
  await worker.step();
  assert.deepEqual(sent, ['env_saved']); assert.equal(plans, 0, 'accepted replay must be read back before replanning');
  await worker.step();
  assert.deepEqual(sent, ['env_saved']); assert.equal(plans, 1);
});

test('closed Conversation preserves pending evidence without planning or resending', async () => {
  const pending = { requests: [{ id: 'env_pre_close' }], next: 0, results: [] };
  let saved;
  const worker = new ConferenceWorker({ cid: 'conv_retired',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'closed', events: [], capabilities: {}, participants: [
      { participant_id: 'owner', role: 'owner' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ] }), request: async () => assert.fail('closed Conversation cannot resume effects') },
    storage: { load: async () => ({ cid: 'conv_retired', cursor: 0, events: [], needsDecision: true, pending }), save: async s => { saved = structuredClone(s); } },
    planner: { decide: async () => assert.fail('closed Conversation cannot plan') },
  });
  await worker.step();
  assert.equal(worker.closed, true); assert.deepEqual(saved.pending, pending);
});

test('voice-only mode rejects SMS and final notifications even when the server advertises messaging', async () => {
  let context; const sent = [];
  const worker = new ConferenceWorker({ cid: 'conv_voice', mode: 'voice-only',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'open', capabilities: { sms_configured: true, delivery: ['chat', 'sms'], assistant_voice: true, operations: ['session.invite', 'session.end'] }, events: [
      { seq: 1, event_type: 'message.accepted', payload: { from: 'owner', body: 'Coordinate by voice' } },
    ], participants: [
      { participant_id: 'owner', role: 'owner', sms: '+14155550101' },
      { participant_id: 'organizer', role: 'organizer', sms: '+14155550104', sip: 'sip:fixture@example.invalid' },
      { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ] }), command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async request => { sent.push(request); return { type: 'ack' }; } },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async () => {} },
    planner: { decide: async value => { context = value; return { actions: [] }; } },
  });
  await worker.step();
  assert.equal(context.demo_mode, 'voice-only'); assert.equal(context.capabilities.sms_configured, false);
  assert.deepEqual(context.participants.map(p => p.available_channels), [['chat'], ['voice'], ['chat']]);
  assert.throws(() => worker.prepare({ actions: [{ type: 'message', to: ['organizer'], body: 'Send anyway', delivery: 'sms' }] }), /SMS is deferred/);
  assert.throws(() => worker.prepare({ actions: [{ type: 'final_updates' }] }), /SMS is deferred/);
  worker.state.proposal = { id: 'p1', summary: 'Sandbox arrangements' }; worker.state.approved = { proposal_id: 'p1' };
  worker.state.events.push(...confirmedVoice());
  worker.state.pending = worker.prepare({ actions: [{ type: 'complete_voice_rehearsal', proposal_id: 'p1' }] });
  await worker.executePending();
  assert.equal(sent.length, 1); assert.equal(sent[0].payload.delivery, 'chat');
  assert.deepEqual(sent[0].payload.to, ['owner']);
  assert.equal(JSON.parse(sent[0].payload.body).sms, 'deferred');
  assert.equal(worker.state.completedVoiceProposal, 'p1'); assert.equal(worker.state.completedProposal, undefined);
  assert.throws(() => worker.prepare({ actions: [{ type: 'complete_voice_rehearsal', proposal_id: 'p1' }] }), /new owner-approved/);
});

test('an ended failed organizer call cannot become an approved pickup or completed voice demo', () => {
  const worker = new ConferenceWorker({ cid: 'conv_confirmation_failure', mode: 'voice-only', client: {
    command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }),
  }, storage: {} });
  worker.members = [{ role: 'owner', participant_id: 'owner' }, { role: 'organizer', participant_id: 'organizer' }];
  worker.state = { cursor: 10, events: confirmedVoice(), proposal: { id: 'p1', summary: 'Sandbox arrangements' }, approved: { proposal_id: 'p1' } };
  const proposal = { actions: [{ type: 'propose_arrangements', summary: 'Pickup confirmed' }] };
  const complete = { actions: [{ type: 'complete_voice_rehearsal', proposal_id: 'p1' }] };
  assert.equal(worker.prepare(proposal).requests.length, 1);
  const valid = structuredClone(worker.state.events);
  for (const missing of ['session.transcript', 'browser.speaking', 'phone.speaking', 'session.ended']) {
    worker.state.events = valid.filter(e => e.event_type !== missing);
    assert.throws(() => worker.prepare(proposal), /Voice rehearsal requires/);
    assert.throws(() => worker.prepare(complete), /Voice rehearsal requires/);
  }
  worker.state.events = [...valid, { event_type: 'session.assistant_failed', payload: { sid: 'ses_confirm' } }];
  assert.throws(() => worker.prepare(proposal), /failed call does not confirm pickup/);
  worker.members.push({ role: 'booker', participant_id: 'booker' });
  worker.state.events = [...valid,
    { event_type: 'session.invited', payload: { sid: 'ses_booker', participant_id: 'booker' } },
    { event_type: 'session.assistant_failed', payload: { sid: 'ses_booker' } },
    { event_type: 'session.ended', payload: { sid: 'ses_booker' } },
  ];
  assert.throws(() => worker.prepare(proposal), /Voice rehearsal requires/);
  assert.throws(() => worker.prepare(complete), /Voice rehearsal requires/);
  worker.state.events = valid.map(e => e.event_type === 'phone.speaking' ? { ...e, payload: { ...e.payload, participant_id: 'other_owner' } } : e);
  assert.throws(() => worker.prepare(proposal), /Voice rehearsal requires/);
});

test('organizer retries require the latest call to complete both handoffs', () => {
  const worker = new ConferenceWorker({ cid: 'conv_retry', mode: 'voice-only', client: {
    command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }),
  }, storage: {} });
  worker.members = [{ role: 'owner', participant_id: 'owner' }, { role: 'organizer', participant_id: 'organizer' }];
  const retry = confirmedVoice().map(e => ({ ...e, payload: { ...e.payload, sid: 'ses_retry',
    ...(e.payload.connid ? { connid: e.payload.connid === 'conn_organizer' ? 'conn_retry_organizer' : 'conn_retry_browser' } : {}),
    ...(e.payload.details ? { details: { ...e.payload.details, retained_connid: 'conn_retry_organizer',
      ...(e.payload.details.retired_connid ? { retired_connid: 'conn_retry_browser' } : {}) } } : {}),
  } }));
  worker.state = { cursor: 20, proposal: { id: 'p1', summary: 'Sandbox arrangements' }, approved: { proposal_id: 'p1' },
    events: [...confirmedVoice(), { event_type: 'connection.failed', payload: { sid: 'ses_confirm', connid: 'conn_organizer' } }, ...retry] };
  const proposal = { actions: [{ type: 'propose_arrangements', summary: 'Pickup confirmed on retry' }] };
  const complete = { actions: [{ type: 'complete_voice_rehearsal', proposal_id: 'p1' }] };
  assert.equal(worker.prepare(proposal).requests.length, 1);
  assert.equal(worker.prepare(complete).requests.length, 1);
  worker.state.events = [...confirmedVoice(), ...retry.filter(e => e.event_type !== 'phone.speaking')];
  assert.throws(() => worker.prepare(proposal), /Voice rehearsal requires/);
  assert.throws(() => worker.prepare(complete), /Voice rehearsal requires/);
  worker.state.events = [...confirmedVoice(), ...retry,
    { event_type: 'connection.failed', payload: { sid: 'ses_retry', connid: 'conn_retry_organizer' } }];
  assert.throws(() => worker.prepare(proposal), /Voice rehearsal requires/);
  assert.throws(() => worker.prepare(complete), /Voice rehearsal requires/);
});

test('voice-only restart cannot resume an SMS batch from a full-demo state', async () => {
  const state = { cursor: 3, events: [], pending: { requests: [{ type: 'message.send', payload: { delivery: 'sms' } }], next: 0 } };
  const worker = new ConferenceWorker({ cid: 'conv_switch', mode: 'voice-only',
    storage: { load: async () => state, save: async () => {} }, client: { request: async () => assert.fail('SMS cannot escape the mode gate') },
  });
  await assert.rejects(worker.init(), /mode changed/);
  worker.state = state;
  await assert.rejects(worker.executePending(), /Saved SMS/);
});

test('voice recipient shape feedback corrects Vapi singleton arrays before any call is submitted', async () => {
  let attempts = 0; const sent = [];
  const worker = new ConferenceWorker({ cid: 'conv_voice_shape', mode: 'voice-only',
    client: { identity: 'ai', snapshot: async () => ({ state: 'open', capabilities: { assistant_voice: true, operations: ['session.invite'] }, events: [
      { seq: 1, event_type: 'message.accepted', payload: { from: 'owner', body: 'Call the booker' } },
    ], participants: [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'booker', role: 'booker', sip: 'sip:fixture@example.invalid' }, { participant_id: 'ai', role: 'assistant', subject: 'ai' }] }),
    command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async request => { sent.push(request); return { type: 'ack' }; } },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async () => {} },
    planner: { decide: async context => {
      attempts++;
      if (attempts === 1) return { actions: [{ type: 'call_participant', to: ['booker'], purpose: 'Find a sandbox itinerary' }] };
      assert.match(context.validation_feedback.reason, /one participant ID string, not an array/);
      assert.equal(sent.length, 0);
      return { actions: [{ type: 'call_participant', to: 'booker', purpose: 'Find a sandbox itinerary' }] };
    } },
  });
  await worker.step(); assert.equal(attempts, 2); assert.equal(sent.length, 1);
  assert.equal(sent[0].payload.to, 'booker'); assert.equal(sent[0].type, 'session.invite');
});

test('booker call waits through the AI confirmation and its quiet interval across restart', async () => {
  let now = 0, plans = 0;
  const events = [
    { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_booker' } },
    { seq: 2, event_type: 'session.transcript', payload: { sid: 'ses_booker', speaker: 'booker', text: 'Two seats, departure eleven, arrival four.' } },
    { seq: 3, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'booker', state: 'stopped' } },
    { seq: 4, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'ai', state: 'started' } },
  ];
  let saved = { cursor: 0, events: [], pending: null };
  const options = { cid: 'conv_ai_confirmation', now: () => now,
    client: { identity: 'assistant', snapshot: async (_cid, cursor) => ({ state: 'open', capabilities: {}, events: events.filter(e => e.seq > cursor), participants: [
      { participant_id: 'owner', role: 'owner' }, { participant_id: 'booker', role: 'booker' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ] }) },
    storage: { load: async () => structuredClone(saved), save: async state => { saved = structuredClone(state); } },
    planner: { decide: async () => { plans++; return { actions: [] }; } },
  };
  let worker = new ConferenceWorker(options);
  await worker.step(); now = 5000; await worker.step();
  assert.equal(plans, 0, 'human silence does not permit ending during AI speech');
  worker = new ConferenceWorker(options); await worker.step(); assert.equal(plans, 0);
  events.push({ seq: 5, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'ai', state: 'stopped' } });
  await worker.step(); now = 6499; await worker.step(); assert.equal(plans, 0);
  now = 6500; await worker.step(); assert.equal(plans, 1, 'the complete AI readback must finish before another decision');
});

test('speech starting while the planner runs defers the exact end command through confirmation', async () => {
  let now = 5000, plannerCalls = 0, snapshotCalls = 0; const sent = [];
  const members = [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'booker', role: 'booker' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' }];
  const initial = [{ seq: 1, event_type: 'session.invited', payload: { sid: 'ses_booker', participant_id: 'booker' } },
    { seq: 2, event_type: 'session.transcript', payload: { sid: 'ses_booker', speaker: 'booker', text: 'Flight RD742, two seats, sixteen to seventeen, terminal C.' } }];
  const events = [...initial]; let saved = { cursor: 2, events: initial, needsDecision: true, voiceActivityObservedAt: 0 };
  const client = { identity: 'assistant', snapshot: async (_cid, cursor) => {
    snapshotCalls++;
    if (snapshotCalls === 2) events.push({ seq: 3, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'ai', state: 'started' } });
    return { state: 'open', participants: members, capabilities: { operations: ['session.end'] }, events: events.filter(e => e.seq > cursor) };
  }, command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async frame => { sent.push(frame); return { type: 'ack' }; } };
  const options = { client, cid: 'conv_race', now: () => now,
    storage: { load: async () => structuredClone(saved), save: async s => { saved = structuredClone(s); } },
    planner: { decide: async () => { plannerCalls++; return { actions: plannerCalls === 1 ? [{ type: 'end_voice', sid: 'ses_booker' }] : [] }; } },
  };
  let worker = new ConferenceWorker(options); await worker.step();
  assert.equal(sent.length, 0, 'a fresh speech event must stop hangup after planning');
  const exactId = saved.pending.requests[0].id; assert.equal(saved.cursor, 3);
  worker = new ConferenceWorker(options); now = 12000; await worker.step(); assert.equal(sent.length, 0);
  events.push({ seq: 4, event_type: 'session.speech', payload: { sid: 'ses_booker', speaker: 'ai', state: 'stopped' } });
  await worker.step(); now = 13499; await worker.step(); assert.equal(sent.length, 0);
  now = 13500; await worker.step(); assert.equal(sent.length, 1); assert.equal(sent[0].id, exactId);
});

test('organizer acknowledgement starting during planning defers the saved browser invitation across restart', async () => {
  let now = 5000, snapshots = 0; const sent = [];
  const members = [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'organizer', role: 'organizer' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' }];
  const initial = [
    { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_organizer', participant_id: 'organizer', connid: 'conn_organizer' } },
    { seq: 2, event_type: 'session.assistant_attached', payload: { sid: 'ses_organizer' } },
    { seq: 3, event_type: 'session.transcript', payload: { sid: 'ses_organizer', speaker: 'organizer', is_final: true, text: 'Pickup is confirmed. Bring Jonathan in.' } },
  ];
  const events = [...initial]; let saved = { demoMode: 'voice-only', cursor: 3, events: initial, needsDecision: true, voiceActivityObservedAt: 0 };
  const client = { identity: 'assistant', snapshot: async (_cid, cursor) => {
    if (++snapshots === 2) events.push({ seq: 4, event_type: 'session.speech', payload: { sid: 'ses_organizer', speaker: 'ai', state: 'started' } });
    return { state: 'open', participants: members, capabilities: { browser_handoff: true }, events: events.filter(e => e.seq > cursor) };
  }, command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async frame => { sent.push(frame); return { type: 'ack' }; } };
  const options = { client, cid: 'conv_invitation_race', mode: 'voice-only', now: () => now,
    storage: { load: async () => structuredClone(saved), save: async value => { saved = structuredClone(value); } },
    planner: { decide: async () => ({ actions: [{ type: 'request_browser_join', sid: 'ses_organizer' }] }) },
  };
  let worker = new ConferenceWorker(options); await worker.step();
  assert.equal(sent.length, 0, 'David must finish acknowledging pickup before the browser rings');
  const exactId = saved.pending.requests[0].id;
  worker = new ConferenceWorker(options); now = 12000; await worker.step(); assert.equal(sent.length, 0);
  events.push({ seq: 5, event_type: 'session.speech', payload: { sid: 'ses_organizer', speaker: 'ai', state: 'stopped' } });
  await worker.step(); now = 13500; await worker.step();
  assert.equal(sent.length, 1); assert.equal(sent[0].id, exactId);
  assert.equal(JSON.parse(sent[0].payload.body).sid, 'ses_organizer');
});

test('ended organizer calls skip unsent invitations but reconcile ambiguous and legacy submissions with their exact IDs', async () => {
  for (const phase of ['unsent', 'ambiguous', 'legacy']) {
    const sent = []; let saved, plans = 0;
    const members = [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'organizer', role: 'organizer' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' }];
    const events = [
      { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_organizer', participant_id: 'organizer', connid: 'conn_remote' } },
      { seq: 2, event_type: 'session.assistant_attached', payload: { sid: 'ses_organizer' } },
      { seq: 3, event_type: 'session.transcript', payload: { sid: 'ses_organizer', speaker: 'organizer', is_final: true, text: 'Pickup confirmed.' } },
    ];
    saved = { demoMode: 'voice-only', cursor: 3, events: structuredClone(events), needsDecision: true, voiceActivityObservedAt: 0 };
    const client = { identity: 'assistant', snapshot: async (_cid, cursor) => ({ state: 'open', participants: members,
      capabilities: { browser_handoff: true }, events: events.filter(e => e.seq > cursor) }),
      command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async frame => {
        sent.push(frame);
        if (sent.length === 1) throw new UctpError('UCTP request timed out; outcome may be unknown', { request: frame });
        return { type: 'ack' };
      },
    };
    const options = { client, cid: `conv_${phase}`, mode: 'voice-only', now: () => 10000,
      storage: { load: async () => structuredClone(saved), save: async value => { saved = structuredClone(value); } },
      planner: { decide: async () => {
        plans++;
        if (phase === 'unsent') events.push({ seq: 4, event_type: 'session.speech', payload: { sid: 'ses_organizer', speaker: 'ai', state: 'started' } });
        return { actions: [{ type: 'request_browser_join', sid: 'ses_organizer' }] };
      } },
    };
    let worker = new ConferenceWorker(options);
    if (phase === 'unsent') await worker.step();
    else await assert.rejects(worker.step(), /outcome may be unknown/);
    const exactId = saved.pending.requests[0].id;
    if (phase === 'legacy') delete saved.pending.attempted;
    events.push({ seq: events.length + 1, event_type: 'session.ended', payload: { sid: 'ses_organizer' } });
    worker = new ConferenceWorker(options); await worker.step();
    assert.equal(plans, 1, 'reconciliation must not replan from the old snapshot');
    assert.equal(saved.pending, null);
    if (phase === 'unsent') {
      assert.equal(sent.length, 0); assert.equal(saved.lastBatch.results[0].state, 'skipped');
    } else {
      assert.equal(sent.length, 2); assert.ok(sent.every(frame => frame.id === exactId));
      assert.equal(saved.lastBatch.results[0].response.type, 'ack');
    }
  }
});

test('retired AI speech cannot block owner-controlled completion after a browser handoff', () => {
  const worker = new ConferenceWorker({ cid: 'conv_retired_ai', client: {}, storage: {}, now: () => 10000 });
  worker.self = { participant_id: 'ai' };
  worker.state = { voiceActivityObservedAt: 0, events: [
    { event_type: 'session.invited', payload: { sid: 'ses_organizer' } },
    { event_type: 'session.speech', payload: { sid: 'ses_organizer', speaker: 'ai', state: 'started' } },
    { event_type: 'browser.speaking', payload: { sid: 'ses_organizer' } },
  ] };
  assert.equal(worker.voiceBusy(), false);
});

test('worker reconnects after accepted command loses its reply, replaying one saved ID without replanning', async () => {
  let saved = { cursor: 0, events: [], pending: null }, plans = 0, connections = 0, reads = 0;
  const attempts = [], accepted = new Set(), logs = [];
  const client = { authenticated: true, identity: 'assistant',
    close() { this.authenticated = false; }, async connect() { connections++; this.authenticated = true; },
    snapshot: async (_cid, after) => ({ state: ++reads >= 3 ? 'closed' : 'open', capabilities: {}, participants: [
      { participant_id: 'owner', role: 'owner' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' },
    ], events: after ? [] : [{ seq: 1, event_type: 'message.accepted', payload: { from: 'owner', body: 'Help coordinate' } }] }),
    command: (type, cid, payload, ids) => ({ type, cid, payload, ...ids }),
    request: async request => {
      attempts.push(request.id); accepted.add(request.id);
      if (attempts.length === 1) throw new UctpError('UCTP request timed out; outcome may be unknown', { request });
      return { type: 'ack' };
    },
  };
  const worker = new ConferenceWorker({ cid: 'conv_reconnect', client,
    storage: { load: async () => structuredClone(saved), save: async value => { saved = structuredClone(value); } },
    planner: { decide: async () => { plans++; return { actions: [{ type: 'message', to: ['owner'], delivery: 'chat', body: 'I am coordinating.' }] }; } },
  });
  await runWorkerLoop({ client, worker, pause: async () => {}, log: entry => logs.push(entry) });
  assert.equal(connections, 1); assert.equal(plans, 1); assert.equal(accepted.size, 1);
  assert.equal(attempts.length, 2); assert.equal(attempts[0], attempts[1]);
  assert.equal(saved.pending, null); assert.equal(saved.lastBatch.next, 1);
  assert.equal(logs[0].operation, 'message.send'); assert.equal(logs[0].request_id, attempts[0]); assert.equal(logs[0].retry, true);
});

test('worker bounds transport recovery and never retries a protocol rejection', async () => {
  let reads = 0, connections = 0;
  const client = { authenticated: true, close() { this.authenticated = false; }, async connect() { this.authenticated = true; connections++; } };
  const worker = { step: async () => { reads++; throw new UctpError('UCTP request timed out; outcome may be unknown', { request: { type: 'conversation.subscribe' } }); } };
  await assert.rejects(runWorkerLoop({ client, worker, pause: async () => {}, log: () => {} }), /timed out/);
  assert.equal(reads, 4); assert.equal(connections, 3);
  reads = 0; connections = 0;
  worker.step = async () => { reads++; throw new UctpError('Permission denied', { code: 403 }); };
  await assert.rejects(runWorkerLoop({ client, worker, pause: async () => {}, log: () => {} }), /Permission denied/);
  assert.equal(reads, 1); assert.equal(connections, 0);
});

test('browser invitation targets the existing organizer call once and cannot ring for the booker or a failed call', () => {
  const worker = new ConferenceWorker({ cid: 'conv_ring', client: { command: (type, cid, payload, ids) => ({ type, cid, payload, ...ids }) }, storage: {} });
  worker.self = { participant_id: 'ai' };
  worker.members = [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'organizer', role: 'organizer' }, { participant_id: 'booker', role: 'booker' }];
  worker.capabilities = { browser_handoff: true };
  worker.state = { cursor: 5, events: [
    { event_type: 'session.invited', payload: { sid: 'ses_org', connid: 'conn_org', participant_id: 'organizer' } },
    { event_type: 'session.assistant_attached', payload: { sid: 'ses_org' } },
    { event_type: 'session.transcript', payload: { sid: 'ses_org', speaker: 'organizer', is_final: true, text: 'Pickup works; please bring Jonathan.' } },
  ] };
  const decision = { actions: [{ type: 'request_browser_join', sid: 'ses_org' }] };
  const [request] = worker.prepare(decision).requests;
  assert.equal(request.type, 'message.send'); assert.deepEqual(request.payload.to, ['owner']); assert.equal(request.payload.delivery, 'chat');
  assert.deepEqual(JSON.parse(request.payload.body), { type: 'travel.browser_invitation', version: 1, sid: 'ses_org', retained_connid: 'conn_org' });
  assert.throws(() => worker.prepare({ actions: [{ type: 'request_browser_join', sid: 'organizer' }] }), /Copy sid exactly.*\["ses_org"\]/);
  worker.state.events.push({ event_type: 'message.accepted', payload: { from: 'ai', body: request.payload.body } });
  assert.throws(() => worker.prepare(decision), /only once/);
  worker.state.events.pop(); worker.state.events.push({ event_type: 'session.assistant_failed', payload: { sid: 'ses_org' } });
  assert.throws(() => worker.prepare(decision), /live organizer/);
  worker.state.events.pop(); worker.state.events[0].payload.participant_id = 'booker';
  assert.throws(() => worker.prepare(decision), /live organizer/);
});

test('voice tool rings the existing organizer call without a second planner request, once and after speech', async () => {
  let now = 10000, plans = 0; const sent = [];
  const members = [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'organizer', role: 'organizer' },
    { participant_id: 'ai', role: 'assistant', subject: 'assistant' }];
  const events = [
    { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_org', participant_id: 'organizer', connid: 'conn_org' } },
    { seq: 2, event_type: 'session.assistant_attached', payload: { sid: 'ses_org' } },
    { seq: 3, event_type: 'session.assistant_actions', payload: { sid: 'ses_org', participant_id: 'ai', source: 'vapi', actions: ['request_browser_join'] } },
  ];
  let saved = { cursor: 0, events: [], pending: null, needsDecision: true };
  const client = { identity: 'assistant', snapshot: async (_cid, cursor) => ({ state: 'open', participants: members,
    capabilities: { browser_handoff: true }, events: events.filter(e => e.seq > cursor) }),
    command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }), request: async frame => {
      sent.push(frame);
      if (sent.length === 1) throw new UctpError('UCTP request timed out; outcome may be unknown', { request: frame });
      events.push({ seq: 8, event_type: 'message.accepted', payload: { ...frame.payload, from: 'ai' } });
      return { type: 'ack' };
    } };
  const options = { client, cid: 'conv_tool', now: () => now,
    storage: { load: async () => structuredClone(saved), save: async state => { saved = structuredClone(state); } },
    planner: { decide: async () => { plans++; assert.fail('the live voice tool owns this decision'); } } };
  let worker = new ConferenceWorker(options); await worker.step();
  assert.equal(sent.length, 0, 'wait for confirmation and the tool, rather than planning from greetings');
  events.push({ seq: 4, event_type: 'session.assistant_action', payload: { sid: 'ses_org', participant_id: 'ai', source: 'vapi', action: 'request_browser_join' } });
  await worker.step(); assert.equal(sent.length, 0, 'a tool without an attributed organizer reply cannot invite');
  events.push({ seq: 5, event_type: 'session.transcript', payload: { sid: 'ses_org', speaker: 'organizer', is_final: true, text: 'Pickup confirmed.' } },
    { seq: 6, event_type: 'session.speech', payload: { sid: 'ses_org', speaker: 'ai', state: 'started' } });
  await worker.step(); now = 20000; await worker.step(); assert.equal(sent.length, 0);
  events.push({ seq: 7, event_type: 'session.speech', payload: { sid: 'ses_org', speaker: 'ai', state: 'stopped' } });
  await worker.step(); now = 21500;
  await assert.rejects(worker.step(), /outcome may be unknown/);
  const exactId = saved.pending.requests[0].id;
  worker = new ConferenceWorker(options); await worker.step(); await worker.step(); await worker.step();
  assert.equal(plans, 0); assert.equal(sent.length, 2, 'only reconcile the ambiguous first submission');
  assert.ok(sent.every(f => f.id === exactId));
  assert.equal(JSON.parse(sent[1].payload.body).retained_connid, 'conn_org');
  assert.equal(saved.lastDecision.decision.source, 'voice-tool');
});

test('retry planning identifies a newer owner task despite a later assistant failure report', async () => {
  let context;
  const events = [
    { seq: 1, event_type: 'phone.failed', payload: { sid: 'old_call' } },
    { seq: 2, event_type: 'message.accepted', payload: { from: 'owner', to: ['ai'], content_type: 'text/plain', body: 'Retry the organizer call.' } },
    { seq: 3, event_type: 'message.accepted', payload: { from: 'ai', to: ['owner'], content_type: 'text/plain', body: 'The earlier call failed.' } },
    { seq: 4, event_type: 'message.accepted', payload: { from: 'owner', to: ['ai'], content_type: 'application/json', body: '{}' } },
  ];
  const worker = new ConferenceWorker({ cid: 'conv_retry_order',
    client: { identity: 'assistant', snapshot: async () => ({ state: 'open', capabilities: {}, events,
      participants: [{ participant_id: 'owner', role: 'owner' }, { participant_id: 'ai', role: 'assistant', subject: 'assistant' }] }) },
    storage: { load: async () => ({ cursor: 0, events: [], pending: null }), save: async () => {} },
    planner: { decide: async value => { context = value; return { actions: [] }; } },
  });
  await worker.step();
  assert.equal(context.latest_owner_request.seq, 2);
  assert.equal(context.latest_voice_failure.seq, 1);
  assert.equal(context.events.length, 4, 'all original history remains available');
});
