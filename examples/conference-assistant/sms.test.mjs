import test from 'node:test';
import assert from 'node:assert/strict';
import { campaignSms } from './sms.mjs';
test('campaign framing covers questions and sandbox updates without repeating disclosures', () => {
  for (const body of ['Can you confirm pickup for the requested demo?', '[Sandbox arrangements] Your requested plan is confirmed.']) {
    const message = campaignSms(body);
    assert.ok(message.startsWith('Rudeless Thelve: '));
    assert.ok(message.endsWith('Reply STOP to opt out.'));
    assert.equal(campaignSms(message), message);
    assert.ok(message.includes(body));
  }
  assert.throws(() => campaignSms(''), /required/);
});
