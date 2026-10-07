import test from 'node:test';
import assert from 'node:assert/strict';
import { workerEnvironment } from './environment.mjs';

test('the external assistant receives no carrier, host administrator or cloud credentials', () => {
  const env = workerEnvironment({ PATH: '/usr/bin', VAPI_PRIVATE_KEY: 'fixture-planning-key',
    PARLEY_API_SECRET: 'fixture-admin', TELNYX_API_KEY: 'fixture-carrier', AWS_ACCESS_KEY_ID: 'fixture-cloud',
    SIP_PASSWORD: 'fixture-trunk', CONFERENCE_OWNER_TOKEN: 'fixture-owner', UCTP_URL: 'wss://wrong.invalid' },
  { cid: 'conv_fixture', token: 'scoped-assistant', url: 'wss://conference.example.invalid/uctp', mode: 'voice-only', statePath: '/tmp/worker.json' });
  assert.equal(env.VAPI_PRIVATE_KEY, 'fixture-planning-key');
  assert.equal(env.CONFERENCE_ASSISTANT_TOKEN, 'scoped-assistant');
  assert.equal(env.UCTP_URL, 'wss://conference.example.invalid/uctp');
  for (const secret of ['fixture-admin', 'fixture-carrier', 'fixture-cloud', 'fixture-trunk', 'fixture-owner'])
    assert.ok(!Object.values(env).includes(secret));
});
