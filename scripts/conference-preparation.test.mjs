import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { prepareConversation } from './conference-preparation.mjs';

async function fixture(t) {
  const root = await mkdtemp(`${tmpdir()}/parley-preparation-`);
  t.after(() => rm(root, { recursive: true, force: true }));
  const participants = ['owner', 'companion', 'booker', 'organizer', 'assistant'].map(role => ({ alias: role, name: role, role }));
  const requests = [], tokens = [], cache = new Map();
  let dropResponse = false, failToken = false, time = 100000, creates = 0;
  const options = { statePath: `${root}/preparation.json`, bundleRoot: root, participants,
    httpUrl: 'http://127.0.0.1:8080', uctpUrl: 'ws://127.0.0.1:7443', adminToken: 'private-admin', now: () => time,
    makeClient: () => ({ connect: async () => {}, close: () => {}, request: async request => {
      assert.deepEqual(JSON.parse(await readFile(options.statePath)).create, request, 'command must be saved before any effect');
      requests.push(structuredClone(request));
      if (!cache.has(request.id)) {
        creates++;
        cache.set(request.id, { type: 'conversation.opened', cid: 'conv_test', payload: { participants: participants.map(p => ({ ...p, participant_id: `par_${p.role}` })) } });
      }
      if (dropResponse) { dropResponse = false; throw new Error('connection lost after acceptance'); }
      return structuredClone(cache.get(request.id));
    } }),
    fetchImpl: async (url, init) => {
      const { participant_id } = JSON.parse(init.body);
      tokens.push(participant_id);
      if (failToken && participant_id === 'par_assistant') return { ok: false, status: 503 };
      return { ok: true, json: async () => ({ token: `scoped_${tokens.length}`, conversation_id: 'conv_test', participant_id, expires_in: 43200 }) };
    },
  };
  return { options, requests, tokens, root, creates: () => creates,
    drop: () => { dropResponse = true; }, fail: value => { failToken = value; }, advance: () => { time += 43200001; } };
}

test('lost create response retries exact saved command; one Conversation', async t => {
  const f = await fixture(t); f.drop();
  await assert.rejects(prepareConversation(f.options), /connection lost/);
  const result = await prepareConversation(f.options);
  assert.equal(result.bundle.cid, 'conv_test');
  assert.equal(f.creates(), 1); assert.deepEqual(f.requests[0], f.requests[1]);
  assert.equal((await stat(f.options.statePath)).mode & 0o777, 0o600);
  assert.equal((await stat(result.bundlePath)).mode & 0o777, 0o600);
  assert.ok(!JSON.stringify(await readFile(f.options.statePath, 'utf8')).includes('private-admin'));
});

test('partial token failure resumes missing token without creating or reissuing owner token', async t => {
  const f = await fixture(t); f.fail(true);
  await assert.rejects(prepareConversation(f.options), /503/);
  f.fail(false);
  const result = await prepareConversation(f.options);
  assert.deepEqual(f.tokens, ['par_owner', 'par_assistant', 'par_assistant']);
  assert.equal(f.requests.length, 1); assert.equal(result.bundle.participants.find(p => p.role === 'owner').token, 'scoped_1');
  const copied = JSON.parse(await readFile(result.bundlePath));
  assert.deepEqual(copied, result.bundle);
  await prepareConversation(f.options);
  assert.equal(f.tokens.length, 3);
});

test('expired or explicitly refreshed tokens keep the same Conversation', async t => {
  const f = await fixture(t); await prepareConversation(f.options);
  f.advance(); await prepareConversation(f.options);
  await prepareConversation({ ...f.options, refreshTokens: true });
  assert.equal(f.tokens.length, 6); assert.equal(f.requests.length, 1);
});

test('changed roster/server refuses to reuse preparation without effects', async t => {
  const f = await fixture(t); await prepareConversation(f.options);
  for (const changed of [{ participants: f.options.participants.map(p => ({ ...p, name: 'Changed' })) }, { uctpUrl: 'wss://other.example/uctp' }]) {
    await assert.rejects(prepareConversation({ ...f.options, ...changed }), /different roster or server/);
  }
  assert.equal(f.requests.length, 1); assert.equal(f.tokens.length, 2);
});

test('concurrent preparation cannot use the same state file', async t => {
  const f = await fixture(t);
  await writeFile(`${f.options.statePath}.lock`, 'other process', { mode: 0o600 });
  await assert.rejects(prepareConversation(f.options), /lock exists/);
  assert.equal(f.requests.length, 0); assert.equal(f.tokens.length, 0);
});

test('rejects insecure remote URLs and mismatched token identity before using credentials', async t => {
  const f = await fixture(t);
  await assert.rejects(prepareConversation({ ...f.options, httpUrl: 'http://remote.example' }), /HTTPS/);
  await assert.rejects(prepareConversation({ ...f.options, uctpUrl: 'ws://remote.example' }), /WSS/);
  await assert.rejects(prepareConversation({ ...f.options, fetchImpl: async () => ({ ok: true, json: async () => ({ token: 'wrong', conversation_id: 'conv_other', participant_id: 'par_owner', expires_in: 43200 }) }) }), /identity/);
  const saved = JSON.parse(await readFile(f.options.statePath));
  assert.ok(saved.bundle.participants.every(p => !p.token));
});
