import test from 'node:test';
import assert from 'node:assert/strict';
import { configurePhones, phoneSettings } from './phone-settings.mjs';
const roster = ['owner', 'booker', 'organizer', 'assistant'].map(role => ({ role, name: role, sip: `sip:+12025550190@sip.example;transport=tcp`, ...(['owner', 'organizer'].includes(role) ? { sms: '+12025550190' } : {}) }));
const values = { owner: '(415) 555-0100', organizer: '+1 415 555 0101', booker: '+44 20 7946 0018' };
test('phone changes preserve carrier routes and update SMS only for owner and organizer', () => {
  const updated = configurePhones(roster, values);
  assert.deepEqual(phoneSettings(updated), { owner: '+14155550100', organizer: '+14155550101', booker: '+442079460018' });
  assert.equal(updated[0].sip, 'sip:+14155550100@sip.example;transport=tcp');
  assert.equal(updated[0].sms, '+14155550100'); assert.equal(updated[2].sms, '+14155550101');
  assert.equal(updated[1].sms, undefined); assert.deepEqual(updated[3], roster[3]);
  assert.equal(roster[0].sms, '+12025550190');
});
test('rejects missing numbers, SIP destinations, extra fields and absent provisioned carrier routes', () => {
  for (const invalid of [null, [], {}, { ...values, owner: 'sip:+14155550100@attacker.example' }, { ...values, owner: '' }, { ...values, admin: true }]) assert.throws(() => configurePhones(roster, invalid));
  assert.throws(() => configurePhones([{role:'owner'}], values), /route is not configured/);
});
