import test from 'node:test';
import assert from 'node:assert/strict';
import { requireFullSms } from './conference-worker-readiness.mjs';
const members = ['owner', 'companion', 'booker', 'organizer'].map((role, i) => ({ role, participant_id: role, sms: `+1415555010${i}` }));
const payload = { capabilities: { sms_mode: 'telnyx', sms_configured: true }, sms_eligibility: members.map(m => ({ participant_id: m.participant_id, eligible: true })) };
test('full worker refuses unreviewed, incomplete, duplicate and unassigned routes before any task', () => {
  assert.doesNotThrow(() => requireFullSms(members, payload, { configuration_ready: true }));
  assert.throws(() => requireFullSms(members, payload, { configuration_ready: false }), /assignment/);
  assert.throws(() => requireFullSms(members, { ...payload, sms_eligibility: [] }, { configuration_ready: true }), /enrollment/);
  const incomplete = structuredClone(members); delete incomplete[3].sms;
  assert.throws(() => requireFullSms(incomplete, payload, { configuration_ready: true }), /organizer/);
  const mixed = structuredClone(members); delete mixed[1].sms; delete mixed[2].sms;
  assert.doesNotThrow(() => requireFullSms(mixed, payload, { configuration_ready: true }));
  const duplicate = structuredClone(members); duplicate[1].sms = duplicate[0].sms;
  assert.throws(() => requireFullSms(duplicate, payload, { configuration_ready: true }), /distinct/);
  assert.throws(() => requireFullSms(members, { ...payload, capabilities: { sms_mode: 'fake', sms_configured: true } }, { configuration_ready: true }), /Telnyx/);
});
