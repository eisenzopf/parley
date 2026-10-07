import test from 'node:test';
import assert from 'node:assert/strict';
import { ConversationProjection, stageText } from './projection.mjs';
const members = ['owner', 'companion', 'booker', 'organizer', 'assistant'].map(role => ({ participant_id: role, role, name: role }));
function fixture() {
  const model = new ConversationProjection('conv_test', members); let seq = 0;
  return { model, add: (event_type, payload) => { const e = { cid: 'conv_test', seq: ++seq, event_type, payload }; model.apply(e); return e; } };
}
const edge = (model, id) => model.network(true, 'fake').edges.find(e => e.id === id);

test('phone callback keeps browser audio until a confirmed retained-call replacement, then shows two telephone legs', () => {
  const { model, add } = fixture();
  add('session.invited', { sid: 's1', connid: 'jeff', participant_id: 'organizer' });
  add('session.assistant_attached', { sid: 's1', connid: 'vapi' });
  add('browser.speaking', { sid: 's1', connid: 'web', state: 'speaking', details: { retained_connid: 'jeff', bridge_id: 'b1' } });
  for (const state of ['prepared', 'dialing', 'answered', 'confirmed']) add(`phone.${state}`, { sid: 's1', connid: 'phone', state });
  assert.equal(edge(model, 'browser').state, 'speaking'); assert.equal(edge(model, 'phone').state, 'confirmed');
  add('phone.failed', { sid: 's1', connid: 'phone', state: 'failed' });
  assert.equal(edge(model, 'browser').state, 'speaking');
  add('phone.speaking', { sid: 's1', connid: 'phone2', state: 'speaking', details: { retained_connid: 'wrong', retired_connid: 'web', join_confirmed: true } });
  assert.equal(edge(model, 'phone').state, 'unknown'); assert.equal(edge(model, 'browser').state, 'speaking');
  add('phone.speaking', { sid: 's1', connid: 'phone3', state: 'speaking', details: { retained_connid: 'jeff', retired_connid: 'web', join_confirmed: true, bridge_id: 'b2' } });
  assert.equal(model.voice.sid, 's1'); assert.equal(model.voice.remote, 'jeff'); assert.equal(model.voice.bridge, 'b2');
  assert.equal(edge(model, 'browser').state, 'retired'); assert.equal(edge(model, 'phone').label, 'SIP/PSTN · bridge active');
  assert.equal(edge(model, 'rtp').state, 'active');
  add('session.ended', { sid: 's1', state: 'ended' });
  add('phone.speaking', { sid: 's1', connid: 'late', state: 'speaking', details: { retained_connid: 'jeff', retired_connid: 'web', join_confirmed: true } });
  assert.equal(edge(model, 'phone').state, 'ended'); assert.equal(edge(model, 'rtp').state, 'ended');
});

test('connection projection retains the telephone ID and distinguishes signaling from a bridge', () => {
  const { model, add } = fixture();
  add('session.invited', { sid: 's1', connid: 'telephone', participant_id: 'organizer', initiated_by: 'assistant' });
  add('connection.connected', { sid: 's1', connid: 'telephone', state: 'connected' });
  assert.equal(edge(model, 'sip').state, 'connected');
  assert.equal(edge(model, 'rtp').state, 'idle');
  add('session.assistant_attached', { sid: 's1', connid: 'vapi', bridge_id: 'bridge_ai' });
  add('connection.offered', { sid: 's1', connid: 'browser', transport: 'webrtc' });
  add('browser.failed', { sid: 's1', connid: 'browser', state: 'failed' });
  assert.equal(edge(model, 'vapi').state, 'active', 'failed browser preparation must not imply retired AI');
  add('browser.answered', { sid: 's1', connid: 'browser2', state: 'answered' });
  assert.equal(edge(model, 'browser').label, 'WebRTC · media ready');
  add('browser.speaking', { sid: 's1', connid: 'browser2', state: 'speaking', details: { retained_connid: 'telephone', bridge_id: 'bridge_human' } });
  assert.equal(model.voice.remote, 'telephone'); assert.equal(model.voice.retained, true);
  assert.equal(edge(model, 'vapi').state, 'retired');
  assert.equal(edge(model, 'browser').label, 'WebRTC · bridge active');
  add('session.ended', { sid: 's1', state: 'ended' });
  add('session.assistant_attached', { sid: 's1', connid: 'late' });
  assert.equal(model.voice.state, 'ended', 'late attachment cannot resurrect an ended call');
  assert.equal(edge(model, 'rtp').state, 'ended');
  add('session.invited', { sid: 's2', connid: 'second_phone', participant_id: 'booker', initiated_by: 'owner' });
  assert.equal(model.voice.retained, undefined);
  assert.equal(edge(model, 'browser').state, 'idle');
  assert.equal(edge(model, 'browser').seq, undefined, 'a new Session cannot reuse old connection evidence');
});

test('foreign or repeated events cannot alter the view and a different retained ID is unverified', () => {
  const { model, add } = fixture();
  const invitation = add('session.invited', { sid: 's1', connid: 'phone', participant_id: 'organizer' });
  assert.equal(model.apply({ ...invitation, cid: 'another', seq: 99 }), false);
  assert.equal(model.apply(invitation), false);
  add('session.assistant_attached', { sid: 's1', connid: 'vapi' });
  add('browser.speaking', { sid: 's1', connid: 'web', state: 'speaking', details: { retained_connid: 'different' } });
  assert.equal(model.voice.retained, false); assert.equal(edge(model, 'browser').state, 'unknown');
  assert.equal(edge(model, 'vapi').state, 'active');
  add('session.interrupted', { sid: 's1' });
  assert.equal(edge(model, 'rtp').state, 'unknown');
});

test('final updates require attributed owner approval and delivery never means human confirmation', () => {
  const { model, add } = fixture();
  const msg = (from, body) => add('message.accepted', { from, to: ['assistant'], delivery: 'chat', content_type: 'application/json', body: JSON.stringify(body) });
  msg('assistant', { type: 'travel.proposal', id: 'p1', summary: 'Sandbox trip' });
  msg('assistant', { type: 'travel.approval', version: 1, approved: true, proposal_id: 'p1' });
  assert.equal(model.approval, null); assert.deepEqual(model.finalUpdates(), []);
  msg('owner', { type: 'travel.approval', version: 1, approved: true, proposal_id: 'p1' });
  for (const member of members.slice(0, 4)) {
    add('message.accepted', { msg_id: `msg_${member.role}`, from: 'assistant', to: [member.participant_id], delivery: 'sms', body: '[Sandbox arrangements] Updated.', deliveries: [{ id: member.role, participant_id: member.participant_id, state: 'queued' }] });
    add('message.delivery', { delivery_id: member.role, participant_id: member.participant_id, state: 'sent' });
  }
  assert.equal(model.finalUpdates().filter(u => u.delivery.state === 'sent').length, 4);
  assert.equal(model.finalUpdates().filter(u => u.delivery.state === 'delivered').length, 0);
  assert.equal(model.facts.length, 0);
  msg('owner', { type: 'travel.approval', version: 1, approved: true, proposal_id: 'p1' });
  assert.equal(model.finalUpdates().filter(u => u.delivery.state === 'sent').length, 4);
  add('message.received', { from: 'organizer', body: 'Confirmed' });
  assert.equal(model.facts.length, 1);
  assert.equal(stageText('Call +1 (415) 555-0123 for flight 742 at 17:00.'), 'Call [phone number] for flight 742 at 17:00.');
});

test('voice-only completion requires the approved proposal and never implies SMS delivery', () => {
  const { model, add } = fixture();
  const msg = (from, body) => add('message.accepted', { from, to: ['owner'], delivery: 'chat', content_type: 'application/json', body: JSON.stringify(body) });
  msg('assistant', { type: 'travel.proposal', id: 'p1', summary: 'Sandbox trip' });
  const completion = { type: 'travel.voice_complete', version: 1, proposal_id: 'p1', sms: 'deferred' };
  msg('assistant', completion); assert.equal(model.voiceComplete, null);
  msg('owner', { type: 'travel.approval', version: 1, approved: true, proposal_id: 'p1' });
  msg('assistant', { ...completion, proposal_id: 'wrong' }); assert.equal(model.voiceComplete, null);
  msg('assistant', completion); assert.ok(model.voiceComplete);
  assert.equal(model.deliveries.size, 0);
  assert.equal(model.network(true, 'deferred').edges.find(e => e.id === 'sms').state, 'deferred');
});

test('only an assistant invitation for the current live organizer call enables browser answer', () => {
  const { model, add } = fixture();
  const invitation = (from, sid, retained_connid) => add('message.accepted', { from, to: ['owner'], delivery: 'chat', content_type: 'application/json', body: JSON.stringify({ type: 'travel.browser_invitation', version: 1, sid, retained_connid }) });
  add('session.invited', { sid: 'booker-call', connid: 'booker-leg', participant_id: 'booker' });
  add('connection.connected', { sid: 'booker-call', connid: 'booker-leg', state: 'connected' });
  add('session.assistant_attached', { sid: 'booker-call', connid: 'ai-booker' });
  invitation('assistant', 'booker-call', 'booker-leg'); assert.equal(model.browserInvitation, null);
  add('session.ended', { sid: 'booker-call', state: 'ended' });
  add('session.invited', { sid: 'organizer-call', connid: 'organizer-leg', participant_id: 'organizer' });
  add('connection.connected', { sid: 'organizer-call', connid: 'organizer-leg', state: 'connected' });
  add('session.assistant_attached', { sid: 'organizer-call', connid: 'ai-organizer' });
  assert.equal(model.browserInvitation, null, 'answering and AI attachment alone must not ring');
  invitation('booker', 'organizer-call', 'organizer-leg'); assert.equal(model.browserInvitation, null);
  invitation('assistant', 'organizer-call', 'wrong-leg'); assert.equal(model.browserInvitation, null);
  invitation('assistant', 'organizer-call', 'organizer-leg'); assert.equal(model.browserInvitation.sid, 'organizer-call');
  add('browser.speaking', { sid: 'organizer-call', connid: 'web', state: 'speaking', details: { retained_connid: 'organizer-leg' } });
  assert.equal(model.browserInvitation, null, 'ring stops after actual handoff');
  add('session.ended', { sid: 'organizer-call', state: 'ended' });
  invitation('assistant', 'organizer-call', 'organizer-leg'); assert.equal(model.browserInvitation, null, 'late invitation cannot revive an ended call');
});
