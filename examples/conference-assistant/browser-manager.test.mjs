import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import { BrowserConversationManager } from './browser-manager.mjs';

async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'parley-browser-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const participants = ['owner', 'companion', 'booker', 'organizer', 'assistant'].map(role => ({ alias: role, name: role, role }));
  let now = Date.now(), created = 0, launches = 0, stops = 0, active = 0, pendingSms = 0, mint = 0;
  const records = new Map(), commands = [], prep = new Map();
  const bundle = cid => ({ cid, url: 'ws://127.0.0.1:7443', participants: participants.map(p => ({ ...p,
    participant_id: `${p.role}_${cid}`, subject: `${p.role}_${cid}`, token: `${p.role}-${cid}`, token_expires_at: now + 43200000 })) });
  const options = { stateDir: directory, participants, origin: 'https://conference.example', httpUrl: 'http://127.0.0.1:8080',
    uctpUrl: 'ws://127.0.0.1:7443', adminToken: 'private-admin-fixture', now: () => now,
    provision: async args => { if (!prep.has(args.statePath)) { const b = bundle(`conv_${++created}`); prep.set(args.statePath, b); records.set(b.cid, 'open'); }
      assert.deepEqual(args.participants, participants); return { bundle: prep.get(args.statePath) }; },
    makeClient: (_url, token) => { const cid = token.match(/conv_\d+/)[0]; return { connect: async () => {}, close: () => {},
      snapshot: async () => ({ state: records.get(cid), participants: bundle(cid).participants }),
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
  const grant = await (await http('grants', { method: 'POST', headers: { authorization: 'Bearer private-admin-fixture' } })).json();
  const login = await http('session', { method: 'POST', headers: { origin: options.origin, 'content-type': 'application/json' }, body: JSON.stringify({ grant: grant.grant }) });
  const cookie = login.headers.get('set-cookie').split(';')[0];
  const post = (id, origin = options.origin) => http('conversations', { method: 'POST', headers: { cookie, origin, 'content-type': 'application/json' }, body: JSON.stringify({ request_id: id, participants: [{ role: 'attacker', sms: '+14155559999' }] }) });
  return { manager, options, http, post, cookie, grant, login, commands, records,
    counts: () => ({ created, launches, stops, mint }), active: v => { active = v; }, sms: v => { pendingSms = v; }, advance: ms => { now += ms; } };
}

test('browser access is single-use, persistent, HttpOnly/Secure, and does not expose administrative credentials', async t => {
  const f = await fixture(t);
  assert.match(f.login.headers.get('set-cookie'), /HttpOnly; SameSite=Strict/); assert.match(f.login.headers.get('set-cookie'), /Secure/);
  assert.equal((await f.http('session')).status, 401);
  const duplicate = await f.http('session', { method: 'POST', headers: { origin: f.options.origin }, body: JSON.stringify({ grant: f.grant.grant }) });
  assert.equal(duplicate.status, 401);
  const data = await (await f.http('session', { headers: { cookie: f.cookie } })).json();
  assert.equal(data.authenticated, true); assert.equal(data.current, null);
  assert.ok(!JSON.stringify(data).includes(f.options.adminToken));
  const saved = JSON.parse(await readFile(join(f.options.stateDir, 'browser-access.json'), 'utf8'));
  assert.ok(!JSON.stringify(saved.grants).includes(f.grant.grant)); assert.ok(!JSON.stringify(saved.sessions).includes(f.cookie.slice('parley_browser='.length)));
  f.advance(31 * 86400000); assert.equal((await f.http('session', { headers: { cookie: f.cookie } })).status, 401);
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
