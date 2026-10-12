/** Browser access and worker supervision. Communications still use scoped UCTP. */
import { createServer } from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { readFile, writeFile, mkdir, rename, open, unlink } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { once } from 'node:events';
import { prepareConversation } from '../../scripts/conference-preparation.mjs';
import { UctpClient } from '../../clients/uctp-js/client.mjs';
import { workerEnvironment } from './environment.mjs';
import { checkTelnyxSms } from '../../scripts/preflight-telnyx-sms.mjs';
import { requireFullSms } from '../../scripts/conference-worker-readiness.mjs';
import { validatePasswordVerifier, verifyPassword } from './browser-password.mjs';

const prefix = '/v1/conference/browser';
const cookieName = 'parley_browser';
const hash = value => createHash('sha256').update(value).digest('hex');
const secret = () => randomBytes(32).toString('hex');
const fail = (status, message) => Object.assign(new Error(message), { status });
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
export const RECOVERY_INTERVAL_MS = 5000;

export class BrowserConversationManager {
  constructor({ stateDir, participants, origin, httpUrl, uctpUrl, adminToken, passwordVerifier, environment = process.env,
    initialBundle, initialWorkerState, mode = 'full', now = () => Date.now(), fetchImpl = fetch,
    provision = prepareConversation, makeClient = (url, token) => new UctpClient(url, token), launchWorker, providerCheck }) {
    if (!adminToken || !stateDir || !Array.isArray(participants)) throw new Error('Browser manager private configuration required');
    const site = new URL(origin);
    if (site.origin !== origin || (site.protocol !== 'https:' && !['localhost', '127.0.0.1'].includes(site.hostname))) throw new Error('Trusted browser origin required');
    if (!['full', 'voice-only'].includes(mode)) throw new Error('Unknown browser manager mode');
    Object.assign(this, { stateDir: resolve(stateDir), participants, origin, httpUrl, uctpUrl, adminToken,
      environment, initialBundle, initialWorkerState, mode, now, fetchImpl, provision, makeClient });
    this.passwordVerifier = validatePasswordVerifier(passwordVerifier);
    this.credentialId = hash(JSON.stringify(this.passwordVerifier));
    this.loginAttempts = [];
    this.launchWorker = launchWorker || (bundle => this.spawnWorker(bundle));
    this.providerCheck = providerCheck || (async () => checkTelnyxSms(JSON.parse(await readFile(this.environment.CONFERENCE_BROWSER_SENDER_CONFIG, 'utf8')),
      { apiKey: this.environment.TELNYX_API_KEY || this.environment.TELNYX_TEST_API_KEY }));
    this.queue = Promise.resolve(); this.worker = null;
  }
  serial(action) {
    const next = this.queue.then(action);
    this.queue = next.catch(() => {});
    return next;
  }
  async init() {
    await mkdir(this.stateDir, { recursive: true, mode: 0o700 });
    this.statePath = resolve(this.stateDir, 'browser-access.json');
    try { this.state = JSON.parse(await readFile(this.statePath, 'utf8')); }
    catch (e) {
      if (e.code !== 'ENOENT') throw new Error('Cannot read browser access state');
      this.state = { version: 1, grants: {}, sessions: {}, current: this.initialBundle || null,
        conversations: [], requests: {}, pending: null, workerFailure: null };
      if (this.initialBundle) {
        this.state.conversations.push({ cid: this.initialBundle.cid, created_at: new Date(this.now()).toISOString() });
        const directory = await this.bundleDirectory(this.initialBundle.cid);
        if (this.initialWorkerState) await this.writePrivate(resolve(directory, 'worker-full.json'), this.initialWorkerState);
      }
      await this.save();
    }
    if (this.state.version !== 1) throw new Error('Unsupported browser access state');
    // Requiring a password also retires sessions and links from the old access-link flow.
    this.state.grants = {};
    this.state.sessions = Object.fromEntries(Object.entries(this.state.sessions).filter(([, session]) =>
      session.credential_id === this.credentialId && session.expires_at > this.now()));
    await this.save();
  }
  async writePrivate(path, value) {
    const temp = `${path}.${secret().slice(0, 12)}.tmp`;
    await writeFile(temp, JSON.stringify(value, null, 2), { mode: 0o600, flag: 'wx' });
    await rename(temp, path);
  }
  async save() { await this.writePrivate(this.statePath, this.state); }
  async bundleDirectory(cid) {
    if (!/^conv_[A-Za-z0-9_-]+$/.test(cid)) throw new Error('Invalid managed Conversation');
    const directory = resolve(this.stateDir, 'conversations', cid);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    return directory;
  }
  sessionKey(headers) {
    const raw = (headers.cookie || '').split(';').map(s => s.trim()).find(s => s.startsWith(`${cookieName}=`))?.slice(cookieName.length + 1);
    return typeof raw === 'string' && /^[a-f0-9]{64}$/.test(raw) ? hash(raw) : '';
  }
  authenticated(headers) {
    const session = this.state.sessions[this.sessionKey(headers)];
    if (!session || session.credential_id !== this.credentialId || session.expires_at <= this.now())
      throw fail(401, 'Sign in with your Parley password.');
    return session;
  }
  requireOrigin(headers) {
    if (headers.origin !== this.origin || (headers['sec-fetch-site'] && !['same-origin', 'none'].includes(headers['sec-fetch-site'])))
      throw fail(403, 'Use this page to manage your conversations.');
  }
  async login(password, headers) {
    // A global bound also works behind the local proxy without trusting client IP headers.
    this.loginAttempts = this.loginAttempts.filter(at => at > this.now() - 60000);
    if (this.loginAttempts.length >= 10) throw fail(429, 'Too many sign-in attempts. Wait a minute and try again.');
    this.loginAttempts.push(this.now());
    if (!await verifyPassword(password, this.passwordVerifier)) throw fail(401, 'Incorrect password. Try again.');
    const session = secret();
    delete this.state.sessions[this.sessionKey(headers)];
    this.state.sessions[hash(session)] = { credential_id: this.credentialId, expires_at: this.now() + 30 * 24 * 60 * 60 * 1000 };
    await this.save();
    return session;
  }
  async refreshBundle(bundle) {
    let assistantRenewed = false;
    for (const member of bundle.participants.filter(p => ['owner', 'assistant'].includes(p.role))) {
      if (member.token && member.token_expires_at > this.now() + 300000) continue;
      const response = await this.fetchImpl(new URL(`/v1/conference/${bundle.cid}/tokens`, this.httpUrl), {
        method: 'POST', redirect: 'error', signal: AbortSignal.timeout(10000),
        headers: { authorization: `Bearer ${this.adminToken}`, 'content-type': 'application/json' },
        body: JSON.stringify({ participant_id: member.participant_id }),
      });
      if (!response.ok) throw fail(503, 'Could not renew browser access. Try again.');
      const value = await response.json();
      if (value.conversation_id !== bundle.cid || value.participant_id !== member.participant_id || !value.token || !(value.expires_in > 0))
        throw fail(503, 'Browser access renewal did not match this conversation.');
      member.token = value.token; member.token_expires_at = this.now() + value.expires_in * 1000;
      if (member.role === 'assistant') assistantRenewed = true;
    }
    if (assistantRenewed && this.worker?.cid === bundle.cid) await this.stopWorker();
    await this.writePrivate(resolve(await this.bundleDirectory(bundle.cid), 'provisioned.json'), bundle);
    await this.save();
    return bundle;
  }
  async withOwner(bundle, action) {
    await this.refreshBundle(bundle);
    const client = this.makeClient(bundle.url, bundle.participants.find(p => p.role === 'owner').token);
    try { await client.connect(); return await action(client); }
    finally { client.close(); }
  }
  async stopWorker() {
    const worker = this.worker; this.worker = null;
    if (worker) await worker.stop();
  }
  async ensureWorker(bundle) {
    if (this.worker?.cid === bundle.cid && this.worker.ready) return;
    const failure = this.state.workerFailure;
    if (failure?.cid === bundle.cid && (failure.blocked || failure.retry_at > this.now())) return;
    await this.stopWorker();
    try {
      await this.refreshBundle(bundle);
      this.worker = await this.launchWorker(bundle);
      this.worker.startedAt = this.now();
    } catch (error) {
      await this.recordWorkerFailure(bundle.cid);
      throw error;
    }
  }
  async recordWorkerFailure(cid) {
    if (this.state.current?.cid !== cid) return;
    const attempts = (this.state.workerFailure?.cid === cid ? this.state.workerFailure.attempts || 0 : 0) + 1;
    this.state.workerFailure = { cid, attempts, retry_at: this.now() + Math.min(60000, RECOVERY_INTERVAL_MS * 2 ** (attempts - 1)),
      blocked: attempts >= 6 };
    await this.save();
  }
  async reconcileWorker() {
    const bundle = this.state.current;
    if (!bundle || this.state.pending) return;
    if (this.worker?.cid === bundle.cid && this.worker.ready) {
      // Clear the crash budget only after sustained health, not merely startup.
      if (this.state.workerFailure && this.now() - this.worker.startedAt >= 300000) {
        this.state.workerFailure = null; await this.save();
      }
      const assistant = bundle.participants.find(p => p.role === 'assistant');
      if (assistant.token_expires_at >= this.now() + 300000) return;
    }
    const snapshot = await this.withOwner(bundle, client => client.snapshot(bundle.cid));
    if (snapshot.state === 'closed') { await this.stopWorker(); return; }
    await this.ensureWorker(bundle);
  }
  async spawnWorker(bundle) {
    const directory = await this.bundleDirectory(bundle.cid), statePath = resolve(directory, 'worker-full.json');
    const lockPath = `${statePath}.lock`;
    try {
      const pid = Number(await readFile(lockPath, 'utf8'));
      try { process.kill(pid, 0); throw fail(503, 'The assistant is already running for this conversation.'); }
      catch (error) { if (error.code !== 'ESRCH') throw error; }
      await unlink(lockPath);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await unlink(`${statePath}.ready.json`).catch(error => { if (error.code !== 'ENOENT') throw error; });
    const output = await open(resolve(directory, 'worker.log'), 'a', 0o600);
    const assistant = bundle.participants.find(p => p.role === 'assistant');
    const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
    const child = spawn(process.execPath, [resolve(root, 'examples/conference-assistant/worker.mjs')], {
      cwd: root, stdio: ['ignore', output.fd, output.fd], env: workerEnvironment(this.environment,
        { cid: bundle.cid, token: assistant.token, url: bundle.url, mode: this.mode, statePath, requireOwnerStart: true }),
    });
    await output.close();
    let stopping = false;
    const worker = { cid: bundle.cid, ready: false, stop: async () => {
      stopping = true;
      if (child.exitCode !== null || child.signalCode !== null) return;
      const ended = once(child, 'exit'); child.kill('SIGTERM');
      await Promise.race([ended, delay(5000).then(() => { if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); })]);
    } };
    child.on('exit', () => {
      worker.ready = false;
      // Startup failures are recorded by ensureWorker; count a later exit once.
      if (!stopping && worker.startedAt !== undefined) this.serial(() => this.recordWorkerFailure(bundle.cid)).catch(() => {});
    });
    child.on('error', () => { worker.ready = false; });
    for (let i = 0; i < 50; i++) {
      if (child.exitCode !== null || child.signalCode !== null) throw fail(503, 'The assistant could not start. Try New conversation again.');
      try {
        const ready = JSON.parse(await readFile(`${statePath}.ready.json`, 'utf8'));
        if (ready.pid === child.pid && ready.cid === bundle.cid) { worker.ready = true; return worker; }
      }
      catch (e) { if (e.code !== 'ENOENT') throw e; }
      await delay(100);
    }
    await worker.stop();
    throw fail(503, 'The assistant is taking too long to connect. Try New conversation again.');
  }
  async view(cid = this.state.current?.cid) {
    let bundle = this.state.current;
    if (cid && !this.state.conversations.some(c => c.cid === cid)) throw fail(403, 'This conversation does not belong to this browser.');
    if (cid && bundle?.cid !== cid) bundle = JSON.parse(await readFile(resolve(await this.bundleDirectory(cid), 'provisioned.json'), 'utf8'));
    const conversations = this.state.conversations.map(c => ({ ...c, current: c.cid === this.state.current?.cid })).reverse();
    if (!bundle) return { authenticated: true, current: null, conversations };
    const snapshot = await this.withOwner(bundle, client => client.snapshot(bundle.cid));
    if (snapshot.state !== 'closed' && bundle.cid === this.state.current?.cid) await this.ensureWorker(bundle);
    const owner = bundle.participants.find(p => p.role === 'owner');
    return { authenticated: true, conversations, pending_request_id: this.state.pending?.request_id || null,
      current: { cid: bundle.cid, url: bundle.url, token: owner.token, owner_name: owner.name,
        state: snapshot.state, worker_ready: this.worker?.cid === bundle.cid && this.worker.ready === true,
        worker_failed: this.state.workerFailure?.cid === bundle.cid && !!this.state.workerFailure.blocked,
        worker_recovering: this.state.workerFailure?.cid === bundle.cid && !this.state.workerFailure.blocked && !this.worker?.ready } };
  }
  async newConversation(requestId) {
    if (!/^[a-f0-9]{32}$/.test(requestId || '')) throw fail(400, 'A new-conversation request ID is required.');
    if (this.state.requests[requestId]) return this.view(this.state.requests[requestId]);
    if (this.state.pending && this.state.pending.request_id !== requestId) throw fail(409, 'A conversation is being created. Retry the pending request.');
    this.state.pending ||= { request_id: requestId, previous: this.state.current?.cid || null };
    await this.save();
    const previous = this.state.current;
    await this.stopWorker();
    if (previous) {
      try {
        await this.withOwner(previous, async client => {
          const snapshot = await client.snapshot(previous.cid);
          if (snapshot.state === 'closed') return;
          const { payload } = await client.request(client.command('conversation.preflight', previous.cid));
          if (payload.readiness?.active_sessions || payload.readiness?.unsettled_sms) throw fail(409, 'End the current call and wait for pending messages before starting a new conversation.');
          await client.request(client.command('conversation.close', previous.cid,
            { verification_note: 'Owner selected New conversation in the browser. No voice or SMS work is pending; history is retained.' }, { id: `env_browser_close_${requestId}` }));
          if ((await client.snapshot(previous.cid)).state !== 'closed') throw fail(503, 'Could not finish the previous conversation. Retry New conversation.');
        });
      } catch (error) {
        if (error.status === 409) { this.state.pending = null; await this.save(); await this.ensureWorker(previous); }
        throw error;
      }
    }
    const prepared = await this.provision({ statePath: resolve(this.stateDir, 'preparations', `${requestId}.json`),
      bundleRoot: resolve(this.stateDir, 'conversations'), participants: this.participants,
      httpUrl: this.httpUrl, uctpUrl: this.uctpUrl, adminToken: this.adminToken, fetchImpl: this.fetchImpl, makeClient: this.makeClient });
    await this.withOwner(prepared.bundle, async client => {
      const { payload } = await client.request(client.command('conversation.preflight', prepared.bundle.cid));
      if (!payload.readiness?.ready_for_new_task) throw fail(409, 'Another conversation still has pending work. Try again after it finishes.');
      const snapshot = await client.snapshot(prepared.bundle.cid);
      if (this.mode === 'full' && payload.capabilities?.sms_enrollment_required) {
        const enrolled = new Set((payload.sms_eligibility || []).filter(e => e.eligible).map(e => e.participant_id));
        if (snapshot.participants.some(p => p.sms && !enrolled.has(p.participant_id))) throw fail(409, 'SMS enrollment needs review before this conversation can start.');
        const provider = await this.providerCheck();
        try { requireFullSms(snapshot.participants, payload, provider); }
        catch { throw fail(409, 'The approved SMS sender or recipient enrollment needs attention before starting.'); }
      }
    });
    this.state.current = prepared.bundle;
    if (!this.state.conversations.some(c => c.cid === prepared.bundle.cid)) this.state.conversations.push({ cid: prepared.bundle.cid, created_at: new Date(this.now()).toISOString() });
    this.state.requests[requestId] = prepared.bundle.cid; this.state.pending = null; this.state.workerFailure = null;
    await this.save();
    return this.view(prepared.bundle.cid);
  }
  async handle(req, res) {
    const url = new URL(req.url, this.origin), path = url.pathname;
    res.setHeader('cache-control', 'no-store'); res.setHeader('referrer-policy', 'no-referrer');
    const respond = (status, body) => { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(body)); };
    try {
      if (req.method === 'POST') this.requireOrigin(req.headers);
      let body = {};
      if (req.method === 'POST') {
        let raw = '';
        for await (const chunk of req) { raw += chunk; if (raw.length > 4096) throw fail(413, 'Request too large.'); }
        try { body = JSON.parse(raw || '{}'); } catch { throw fail(400, 'Invalid request.'); }
      }
      if (req.method === 'POST' && path === `${prefix}/session`) {
        const session = await this.serial(() => this.login(body.password, req.headers));
        res.setHeader('set-cookie', `${cookieName}=${session}; Path=${prefix}; HttpOnly; SameSite=Strict; Max-Age=2592000${this.origin.startsWith('https:') ? '; Secure' : ''}`);
        return respond(200, { authenticated: true });
      }
      this.authenticated(req.headers);
      if (req.method === 'GET' && path === `${prefix}/access`) return respond(200, { authenticated: true });
      if (req.method === 'POST' && path === `${prefix}/logout`) {
        await this.serial(async () => { delete this.state.sessions[this.sessionKey(req.headers)]; await this.save(); });
        res.setHeader('set-cookie', `${cookieName}=; Path=${prefix}; HttpOnly; SameSite=Strict; Max-Age=0${this.origin.startsWith('https:') ? '; Secure' : ''}`);
        return respond(200, { authenticated: false });
      }
      if (req.method === 'GET' && path === `${prefix}/session`) return respond(200, await this.serial(() => this.view(url.searchParams.get('cid') || undefined)));
      if (req.method === 'POST' && path === `${prefix}/conversations`) return respond(200, await this.serial(() => this.newConversation(body.request_id)));
      throw fail(404, 'Not found.');
    } catch (error) {
      if (error.status === 429) res.setHeader('retry-after', '60');
      respond(error.status || 503, { error: error.status ? error.message : 'Could not connect to the conversation service. Please retry.' });
    }
  }
}

async function main() {
  const env = process.env;
  const manager = new BrowserConversationManager({ stateDir: env.CONFERENCE_BROWSER_STATE_DIR,
    participants: JSON.parse(await readFile(env.CONFERENCE_BROWSER_ROSTER, 'utf8')),
    origin: env.CONFERENCE_BROWSER_ORIGIN, httpUrl: env.PARLEY_HTTP_URL || 'http://127.0.0.1:8080',
    uctpUrl: env.UCTP_URL, adminToken: env.PARLEY_API_SECRET, environment: env,
    passwordVerifier: JSON.parse(await readFile(env.CONFERENCE_BROWSER_PASSWORD_FILE, 'utf8')),
    initialBundle: env.CONFERENCE_BROWSER_IMPORT ? JSON.parse(await readFile(env.CONFERENCE_BROWSER_IMPORT, 'utf8')) : undefined,
    initialWorkerState: env.CONFERENCE_BROWSER_IMPORT_WORKER ? JSON.parse(await readFile(env.CONFERENCE_BROWSER_IMPORT_WORKER, 'utf8')) : undefined,
    mode: env.CONFERENCE_DEMO_MODE || 'full' });
  await manager.init();
  const lockPath = resolve(manager.stateDir, 'manager.lock');
  try { const lock = await open(lockPath, 'wx', 0o600); await lock.writeFile(String(process.pid)); await lock.close(); }
  catch (error) {
    if (error.code !== 'EEXIST') throw error;
    const pid = Number(await readFile(lockPath, 'utf8'));
    try { process.kill(pid, 0); throw new Error('Browser manager already running'); }
    catch (e) { if (e.code !== 'ESRCH') throw e; }
    await unlink(lockPath); const lock = await open(lockPath, 'wx', 0o600); await lock.writeFile(String(process.pid)); await lock.close();
  }
  const server = createServer((req, res) => manager.handle(req, res));
  server.requestTimeout = 15000; server.headersTimeout = 15000;
  server.listen(Number(env.CONFERENCE_BROWSER_PORT || 8090), '127.0.0.1');
  await once(server, 'listening');
  console.log(JSON.stringify({ event: 'browser-manager.ready', port: server.address().port }));
  // Coalesce ticks while a request/reconciliation is in flight. All launches
  // share the manager queue and the worker's exclusive on-disk process lock.
  let checking = false;
  const timer = setInterval(() => {
    if (checking) return;
    checking = true;
    manager.serial(() => manager.reconcileWorker())
      .catch(() => console.error(JSON.stringify({ event: 'browser-manager.recovery_failed' })))
      .finally(() => { checking = false; });
  }, RECOVERY_INTERVAL_MS);
  const stop = async () => { clearInterval(timer); server.close(); await manager.stopWorker(); await unlink(lockPath); process.exit(0); };
  process.on('SIGTERM', stop); process.on('SIGINT', stop);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(() => { console.error('Browser manager could not start; inspect private configuration and state.'); process.exitCode = 1; });
