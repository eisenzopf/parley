import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import { BrowserConversationManager, RECOVERY_INTERVAL_MS } from './browser-manager.mjs';
import { createPasswordVerifier } from './browser-password.mjs';

const password = 'fixture browser password';
const passwordVerifier = await createPasswordVerifier(password);

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'parley-browser-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const participants = ['owner', 'companion', 'booker', 'organizer', 'assistant'].map(role => ({ alias: role, name: role, role, ...(['owner', 'booker', 'organizer'].includes(role) ? { sip: `sip:+120255501${{ owner: '90', booker: '92', organizer: '91' }[role]}@sip.example` } : {}), ...(['owner', 'organizer'].includes(role) ? { sms: `+120255501${role === 'owner' ? '90' : '91'}` } : {}) }));
  let now = Date.now(), created = 0, launches = 0, stops = 0, active = 0, pendingSms = 0, mint = 0;
  const records = new Map(), commands = [], prep = new Map();
  const bundle = (cid, roster = participants) => ({ cid, url: 'ws://127.0.0.1:7443', participants: roster.map(p => ({ ...p,
    participant_id: `${p.role}_${cid}`, subject: `${p.role}_${cid}`, token: `${p.role}-${cid}`, token_expires_at: now + 43200000 })) });
  const options = { stateDir: directory, participants, origin: 'https://conference.example', httpUrl: 'http://127.0.0.1:8080',
    uctpUrl: 'ws://127.0.0.1:7443', adminToken: 'private-admin-fixture', passwordVerifier, environment: {}, now: () => now,
    provision: async args => { if (!prep.has(args.statePath)) { const b = bundle(`conv_${++created}`, args.participants); prep.set(args.statePath, b); records.set(b.cid, 'open'); }
      return { bundle: prep.get(args.statePath) }; },
    makeClient: (_url, token) => { const cid = token.match(/conv_\d+/)[0]; return { connect: async () => {}, close: () => {},
      snapshot: async () => ({ state: records.get(cid), participants: [...prep.values()].find(b => b.cid === cid)?.participants || bundle(cid).participants }),
      command: (type, cid, payload, extra) => ({ type, cid, payload, ...extra }),
      request: async frame => { commands.push(frame); if (frame.type === 'conversation.close') { records.set(cid, 'closed'); return { type: 'ack' }; }
        return { payload: { readiness: { active_sessions: active, unsettled_sms: pendingSms, ready_for_new_task: !active && !pendingSms }, capabilities: {} } }; },
    }; },
    fetchImpl: async (url, args) => { mint++; const cid = new URL(url).pathname.split('/')[3]; const pid = JSON.parse(args.body).participant_id;
      return { ok: true, json: async () => ({ conversation_id: cid, participant_id: pid, token: `renewed-${pid}`, expires_in: 43200 }) }; },
    launchWorker: async b => { launches++; return { cid: b.cid, ready: true, stop: async () => { stops++; } }; },
  };
  const manager = new BrowserConversationManager(options); await manager.init();
  const server = createServer((req, res) => manager.handle(req, res)); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(async () => { await manager.stopWorker(); await new Promise(resolve => server.close(resolve)); });
  const url = `http://127.0.0.1:${server.address().port}/v1/conference/browser/`;
  const http = (path, options = {}) => fetch(url + path, options);
  const signIn = (value, origin = options.origin) => http('session', { method: 'POST', headers: { origin, 'content-type': 'application/json' }, body: JSON.stringify({ password: value }) });
  const login = await signIn(password);
  const cookie = login.headers.get('set-cookie').split(';')[0];
  const post = (id, origin = options.origin) => http('conversations', { method: 'POST', headers: { cookie, origin, 'content-type': 'application/json' }, body: JSON.stringify({ request_id: id, participants: [{ role: 'attacker', sms: '+14155559999' }] }) });
  return { manager, options, http, post, cookie, login, signIn, commands, records,
    counts: () => ({ created, launches, stops, mint }), active: v => { active = v; }, sms: v => { pendingSms = v; }, advance: ms => { now += ms; } };
}

test('password access is persistent, HttpOnly/Secure, and does not expose password or administrative credentials', async t => {
  const f = await fixture(t);
  assert.match(f.login.headers.get('set-cookie'), /HttpOnly; SameSite=Strict/); assert.match(f.login.headers.get('set-cookie'), /Secure/);
  assert.equal((await f.http('session')).status, 401);
  const linkLogin = await f.http('session', { method: 'POST', headers: { origin: f.options.origin }, body: JSON.stringify({ grant: 'a'.repeat(64) }) });
  assert.equal(linkLogin.status, 401);
  const data = await (await f.http('session', { headers: { cookie: f.cookie } })).json();
  assert.equal(data.authenticated, true); assert.equal(data.current, null);
  assert.ok(!JSON.stringify(data).includes(f.options.adminToken));
  const saved = JSON.parse(await readFile(join(f.options.stateDir, 'browser-access.json'), 'utf8'));
  assert.deepEqual(saved.grants, {}); assert.ok(!JSON.stringify(saved.sessions).includes(f.cookie.slice('parley_browser='.length)));
  assert.ok(!JSON.stringify(saved).includes(password)); assert.ok(!JSON.stringify(saved).includes(passwordVerifier.hash));
  f.advance(31 * 86400000); assert.equal((await f.http('session', { headers: { cookie: f.cookie } })).status, 401);
});

test('wrong passwords, missing passwords, and cross-site login cannot obtain conversation access', async t => {
  const f = await fixture(t);
  for (const value of ['wrong password', '', undefined, { password }, 'x'.repeat(257)]) {
    const rejected = await f.signIn(value); assert.equal(rejected.status, 401);
    assert.equal(rejected.headers.get('set-cookie'), null);
  }
  assert.equal((await f.signIn(password, 'https://attacker.example')).status, 403);
  assert.equal((await f.http('access')).status, 401);
  assert.equal((await f.http('conversations', { method: 'POST', headers: { origin: f.options.origin }, body: JSON.stringify({ request_id: 'c'.repeat(32) }) })).status, 401);
  assert.equal(f.counts().created, 0);
  assert.equal((await f.http('access', { headers: { cookie: f.cookie } })).status, 200);
  assert.throws(() => new BrowserConversationManager({ ...f.options, passwordVerifier: undefined }), /password configuration/);
});

test('password attempts are bounded and retrying works after the limit window', async t => {
  const f = await fixture(t);
  for (let i = 0; i < 9; i++) assert.equal((await f.signIn('wrong password')).status, 401);
  const limited = await f.signIn(password); assert.equal(limited.status, 429); assert.equal(limited.headers.get('retry-after'), '60');
  f.advance(61000); assert.equal((await f.signIn(password)).status, 200);
});

test('logout, enabling passwords, and password rotation revoke sessions while retaining conversation history', async t => {
  const f = await fixture(t); await f.post('a'.repeat(32));
  const logout = await f.http('logout', { method: 'POST', headers: { cookie: f.cookie, origin: f.options.origin } });
  assert.equal(logout.status, 200); assert.match(logout.headers.get('set-cookie'), /Max-Age=0/);
  assert.equal((await f.http('access', { headers: { cookie: f.cookie } })).status, 401);
  const fresh = await f.signIn(password), cookie = fresh.headers.get('set-cookie').split(';')[0];
  f.manager.state.sessions['a'.repeat(64)] = { expires_at: Date.now() + 86400000 };
  f.manager.state.grants['b'.repeat(64)] = { expires_at: Date.now() + 86400000 };
  await f.manager.save();
  const reloaded = new BrowserConversationManager(f.options); await reloaded.init();
  assert.equal(reloaded.state.sessions['a'.repeat(64)], undefined); assert.deepEqual(reloaded.state.grants, {});
  assert.doesNotThrow(() => reloaded.authenticated({ cookie }));
  const rotated = new BrowserConversationManager({ ...f.options, passwordVerifier: await createPasswordVerifier('rotated fixture password') });
  await rotated.init(); assert.throws(() => rotated.authenticated({ cookie }), /Sign in/);
  assert.deepEqual(rotated.state.sessions, {}); assert.equal(rotated.state.current.cid, 'conv_1'); assert.equal(rotated.state.conversations.length, 1);
});

test('New conversation creates fresh IDs, automatically starts its worker, retains history, and survives reload', async t => {
  const f = await fixture(t);
  const first = await (await f.post('a'.repeat(32))).json(); assert.equal(first.current.cid, 'conv_1'); assert.equal(first.current.worker_ready, true);
  const next = await (await f.post('b'.repeat(32))).json(); assert.equal(next.current.cid, 'conv_2');
  assert.equal(f.records.get('conv_1'), 'closed'); assert.equal(next.conversations.length, 2);
  assert.deepEqual(f.counts(), { created: 2, launches: 2, stops: 1, mint: 0 });
  assert.equal(f.commands.filter(c => c.type === 'conversation.close').length, 1);
  assert.ok(!JSON.stringify(next).includes('assistant-conv')); assert.ok(!JSON.stringify(next).includes(f.options.adminToken));
  const reloaded = new BrowserConversationManager(f.options); await reloaded.init();
  assert.doesNotThrow(() => reloaded.authenticated({ cookie: f.cookie }));
  assert.equal(reloaded.state.current.cid, 'conv_2');
  assert.equal((await f.http('session?cid=conv_not_owned', { headers: { cookie: f.cookie } })).status, 403);
  const previous = await (await f.http('session?cid=conv_1', { headers: { cookie: f.cookie } })).json();
  assert.equal(previous.current.state, 'closed'); assert.equal(previous.current.worker_ready, false);
});

test('duplicate and concurrent New requests create exactly one conversation and preserve the original routing', async t => {
  const f = await fixture(t);
  const responses = await Promise.all([f.post('c'.repeat(32)), f.post('c'.repeat(32))]);
  assert.ok(responses.every(r => r.status === 200));
  assert.equal(f.counts().created, 1); assert.equal(f.counts().launches, 1);
  assert.equal(f.commands.filter(c => c.type === 'session.invite' || c.type === 'message.send').length, 0);
});

test('cross-site requests and live or unsettled work cannot close the current conversation', async t => {
  const f = await fixture(t); await f.post('d'.repeat(32));
  assert.equal((await f.post('e'.repeat(32), 'https://attacker.example')).status, 403);
  f.active(1); assert.equal((await f.post('e'.repeat(32))).status, 409); assert.equal(f.records.get('conv_1'), 'open');
  f.active(0); f.sms(1); assert.equal((await f.post('f'.repeat(32))).status, 409); assert.equal(f.records.get('conv_1'), 'open');
  assert.equal(f.commands.filter(c => c.type === 'conversation.close').length, 0); assert.equal(f.counts().created, 1);
});

test('renewing assistant credentials restarts only that worker and refreshes browser access automatically', async t => {
  const f = await fixture(t); await f.post('1'.repeat(32)); f.advance(43200000 - 100000);
  const session = await (await f.http('session', { headers: { cookie: f.cookie } })).json();
  assert.match(session.current.token, /^renewed-owner_/); assert.equal(session.current.worker_ready, true);
  assert.deepEqual(f.counts(), { created: 1, launches: 2, stops: 1, mint: 2 });
  assert.equal(f.commands.filter(c => c.type === 'session.invite' || c.type === 'message.send').length, 0);
});

test('five-second reconciliation recovers a saved failure without a browser request or new conversation', async t => {
  const f = await fixture(t); await f.post('2'.repeat(32));
  await f.manager.stopWorker();
  await f.manager.recordWorkerFailure('conv_1');
  const reloaded = new BrowserConversationManager(f.options); await reloaded.init();
  t.after(() => reloaded.stopWorker());
  await reloaded.reconcileWorker(); assert.equal(f.counts().launches, 1);
  f.advance(RECOVERY_INTERVAL_MS);
  await Promise.all([reloaded.serial(() => reloaded.reconcileWorker()), reloaded.serial(() => reloaded.reconcileWorker())]);
  assert.equal(f.counts().launches, 2); assert.equal(f.counts().created, 1);
  assert.equal(reloaded.worker.cid, 'conv_1'); assert.equal(reloaded.worker.ready, true);
  assert.equal(f.commands.filter(c => ['session.invite', 'message.send'].includes(c.type)).length, 0);
  f.advance(300000); await reloaded.reconcileWorker(); assert.equal(reloaded.state.workerFailure, null);
});

test('recovery backs off across restart and repeated startup failure becomes visible', async t => {
  const f = await fixture(t); await f.post('3'.repeat(32)); await f.manager.stopWorker();
  let attempts = 0;
  f.manager.launchWorker = async () => { attempts++; throw new Error('fixture startup failure'); };
  for (let i = 1; i <= 6; i++) {
    await assert.rejects(f.manager.reconcileWorker(), /startup failure/);
    assert.equal(f.manager.state.workerFailure.attempts, i);
    await f.manager.reconcileWorker(); assert.equal(attempts, i, 'must honor retry deadline');
    f.advance(60000);
  }
  const reloaded = new BrowserConversationManager({ ...f.options, launchWorker: f.manager.launchWorker }); await reloaded.init();
  await reloaded.reconcileWorker(); assert.equal(attempts, 6);
  assert.equal((await reloaded.view()).current.worker_failed, true);
});

test('legacy failed state resumes, but closed conversations and pending replacements never restart', async t => {
  const f = await fixture(t); await f.post('4'.repeat(32)); await f.manager.stopWorker();
  f.manager.state.workerFailure = { cid: 'conv_1' }; await f.manager.save();
  await f.manager.reconcileWorker(); assert.equal(f.counts().launches, 2);
  await f.manager.stopWorker(); f.records.set('conv_1', 'closed');
  await f.manager.reconcileWorker(); assert.equal(f.counts().launches, 2);
  f.records.set('conv_1', 'open'); f.manager.state.pending = { request_id: 'fixture' };
  await f.manager.reconcileWorker(); assert.equal(f.counts().launches, 2);
  await f.manager.recordWorkerFailure('conv_old');
  assert.equal(f.manager.state.workerFailure.cid, 'conv_1');
});

test('saved phone settings require sign-in, survive restart and affect only new conversations', async t => {
  const f = await fixture(t); await f.post('5'.repeat(32));
  const original = structuredClone(f.manager.state.current);
  const request = (phones, headers = { cookie: f.cookie, origin: f.options.origin }) => f.http('phones', { method: 'POST', headers: { ...headers, 'content-type': 'application/json' }, body: JSON.stringify({ phones }) });
  const phones = { owner: '415-555-0100', booker: '+16505550101', organizer: '+14155550102' };
  assert.equal((await request(phones, { origin: f.options.origin })).status, 401);
  assert.equal((await request(phones, { cookie: f.cookie, origin: 'https://attacker.example' })).status, 403);
  assert.equal((await request({ ...phones, owner: 'sip:evil@elsewhere' })).status, 400);
  assert.equal((await request(phones)).status, 200);
  assert.deepEqual(f.manager.state.current, original);
  const reloaded = new BrowserConversationManager(f.options); await reloaded.init();
  assert.equal(reloaded.configuredParticipants()[0].sms, '+14155550100');
  await f.post('6'.repeat(32));
  const current = f.manager.state.current;
  assert.equal(current.participants.find(p => p.role === 'owner').sip, 'sip:+14155550100@sip.example');
  assert.equal(current.participants.find(p => p.role === 'organizer').sms, '+14155550102');
  assert.equal(current.participants.find(p => p.role === 'booker').sms, undefined);
  assert.equal(f.commands.filter(c => ['session.invite', 'message.send'].includes(c.type)).length, 0);
  f.manager.state.pending = { request_id: 'pending' };
  assert.equal((await request(phones)).status, 409);
});

test('setup synchronizes new SMS numbers and repairs an already pending conversation on retry', async t => {
  const f = await fixture(t);
  const path = join(f.options.stateDir, 'enrollments.json');
  const original = { version: 1, source: 'https://rudeless.ai/sms', sender_number: '+12025550199', campaign_id: 'demo', recipients: [] };
  await writeFile(path, JSON.stringify(original));
  Object.assign(f.manager.environment, { PARLEY_SMS_ENROLLMENT_PATH: path, PARLEY_TELNYX_FROM: original.sender_number, PARLEY_SMS_CAMPAIGN_ID: 'demo' });
  const makeClient = f.manager.makeClient;
  f.manager.makeClient = (...args) => {
    const client = makeClient(...args), request = client.request;
    client.request = async frame => {
      const response = await request(frame);
      if (frame.type === 'conversation.preflight') {
        const enrolled = JSON.parse(await readFile(path, 'utf8')).recipients;
        response.payload.capabilities = { sms_enrollment_required: true, sms_mode: 'telnyx', sms_configured: true };
        response.payload.sms_eligibility = (await client.snapshot()).participants.filter(p => p.sms).map(p => ({
          participant_id: p.participant_id, eligible: enrolled.some(r => r.number === p.sms && r.web_enrollment_confirmed && !r.revoked),
        }));
      }
      return response;
    };
    return client;
  };
  f.manager.providerCheck = async () => ({ configuration_ready: true });
  const phones = { owner: '+14155550100', booker: '+16505550101', organizer: '+14155550102' };
  const response = await f.http('phones', { method: 'POST', headers: { cookie: f.cookie, origin: f.options.origin, 'content-type': 'application/json' }, body: JSON.stringify({ phones }) });
  assert.equal(response.status, 200);
  assert.deepEqual(JSON.parse(await readFile(path, 'utf8')).recipients.map(r => r.number), [phones.owner, phones.organizer]);
  // Reproduce a request stuck on the old version with a missing enrollment.
  await writeFile(path, JSON.stringify(original));
  f.manager.state.pending = { request_id: '7'.repeat(32), previous: null, participants: f.manager.configuredParticipants() };
  await f.manager.save();
  const created = await f.post('7'.repeat(32));
  assert.equal(created.status, 200, await created.text());
  assert.equal(f.manager.state.pending, null);
  const owner = f.manager.state.current.participants.find(p => p.role === 'owner');
  assert.equal(owner.sms, phones.owner);
  assert.equal(owner.sip, `sip:${phones.owner}@sip.example`);
  assert.equal(f.commands.filter(c => ['session.invite', 'message.send'].includes(c.type)).length, 0);
});
