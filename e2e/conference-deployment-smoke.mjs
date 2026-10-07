// Read-only HTTPS/authenticated-UCTP check using an existing private fixture.
// No provision, token issuance, calls, messages, keyword injection or reset.
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { UctpClient } from '../clients/uctp-js/client.mjs';
const [fixturePath, output] = process.argv.slice(2);
if (!fixturePath || !output) throw new Error('Usage: node e2e/conference-deployment-smoke.mjs <existing-private-fixture.json> <new-result.json>');
const fixture = JSON.parse(await readFile(fixturePath, 'utf8'));
const origin = 'https://conference.rudeless.ai';
assert.equal(new URL(fixture.http).origin, origin);
assert.equal(fixture.url, 'wss://conference.rudeless.ai/uctp');
assert.ok(fixture.cid && fixture.token);
const checks = [];
for (const [path, method, expected] of [
  ['/healthz', 'GET', 200], ['/conference/', 'GET', 200],
  [`/v1/conference/${encodeURIComponent(fixture.cid)}/tokens`, 'POST', 401],
  ['/v1/widget/token', 'POST', 404], ['/v1/sms/inbound', 'POST', 401],
]) {
  const response = await fetch(`${origin}${path}`, { method, redirect: 'error', signal: AbortSignal.timeout(10000),
    ...(method === 'POST' ? { headers: { 'content-type': 'application/json' }, body: '{}' } : {}) });
  assert.equal(response.status, expected, `${path}: unexpected HTTP status`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  checks.push({ path: path.includes(fixture.cid) ? '/v1/conference/<existing>/tokens' : path,
    method, status: response.status,
    ...(path === '/conference/' ? { sha256: createHash('sha256').update(bytes).digest('hex') } : {}) });
}
const client = new UctpClient(fixture.url, fixture.token);
try {
  await client.connect();
  const frame = await client.request(client.command('conversation.subscribe', fixture.cid, { after: 0, live: false }));
  assert.equal(frame.type, 'conversation.snapshot');
  assert.equal(frame.cid, fixture.cid);
  const snapshot = frame.payload;
  assert.ok(Array.isArray(snapshot.events));
  assert.ok(snapshot.events.every(event => event.cid === fixture.cid));
  const result = { status: 'passed', checks, authenticated_uctp: true,
    existing_conversation_read: true, conversation_state: snapshot.state,
    journal_event_count: snapshot.events.length,
    no_provider_actions: true, no_state_mutations: true };
  await writeFile(output, JSON.stringify(result, null, 2), { flag: 'wx', mode: 0o600 });
  console.log(JSON.stringify(result));
} finally { client.close(); }
