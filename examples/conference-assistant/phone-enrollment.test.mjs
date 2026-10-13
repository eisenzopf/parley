import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, stat, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { syncPhoneEnrollment } from './phone-enrollment.mjs';

test('setup adds enrollment without changing existing records or clearing withdrawals', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'parley-enrollment-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const path = join(dir, 'records.json');
  const revoked = { number: '+12025550101', revoked: true, evidence: 'withdrawn' };
  const original = { version: 1, source: 'https://rudeless.ai/sms', sender_number: '+12025550199', campaign_id: 'demo', recipients: [revoked] };
  await writeFile(path, JSON.stringify(original));
  const env = { PARLEY_SMS_ENROLLMENT_PATH: path, TELNYX_FROM_NUMBER: original.sender_number, PARLEY_SMS_CAMPAIGN_ID: 'demo' };
  const participants = [{ sms: '+12025550100' }, { sms: revoked.number }, { role: 'booker' }];
  await syncPhoneEnrollment(participants, env);
  const first = await readFile(path, 'utf8'), records = JSON.parse(first);
  assert.deepEqual(records.recipients[0], revoked);
  assert.equal(records.recipients.length, 2);
  assert.equal(records.recipients[1].confirmation_method, 'authenticated_demo_owner');
  assert.equal(records.recipients[1].web_enrollment_confirmed, true);
  assert.equal((await stat(path)).mode & 0o777, 0o600);
  await syncPhoneEnrollment(participants, env);
  assert.equal(await readFile(path, 'utf8'), first);
  await assert.rejects(syncPhoneEnrollment(participants, { ...env, PARLEY_SMS_CAMPAIGN_ID: 'different' }), /configuration/);
  assert.equal(await readFile(path, 'utf8'), first);
});
