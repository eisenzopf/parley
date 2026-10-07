/** Experimental conversation-control/1 reference client; browser + Node 22+. */
export const PROFILE = 'conversation-control/1';

export function envelope(type, payload = {}, ids = {}) {
  return { v: 1, type, id: `env_${crypto.randomUUID().replaceAll('-', '')}`,
    ts: new Date().toISOString(), ...ids, payload };
}

export class UctpError extends Error {
  constructor(message, { code, request, response } = {}) {
    super(message); this.name = 'UctpError';
    this.code = code; this.request = request; this.response = response;
  }
}

// The inspector never receives bearer credentials or auth-session tokens.
export function redact(frame) {
  const copy = structuredClone(frame);
  if (copy.type.startsWith('auth.')) copy.payload = { redacted: true };
  if (copy.signature) copy.signature = '[redacted]';
  function scrub(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      if (['credential', 'password', 'token', 'session_token', 'authorization'].includes(key.toLowerCase())) value[key] = '[redacted]';
      else if (key.toLowerCase() === 'sdp' && typeof child === 'string') value[key] = child.replace(/^(a=(?:ice-pwd|ice-ufrag|crypto|key-mgmt):|k=)[^\r\n]*/gmi, '$1[redacted]');
      else scrub(child);
    }
  }
  scrub(copy);
  return copy;
}

export class UctpClient {
  constructor(url, token, { WebSocketImpl = globalThis.WebSocket, timeoutMs = 10000, trace = () => {} } = {}) {
    const parsed = new URL(url);
    if (!['ws:', 'wss:'].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error('UCTP WebSocket URL required');
    if (parsed.protocol === 'ws:' && !['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname)) throw new Error('Remote UCTP requires wss');
    this.url = url; this.token = token; this.WebSocketImpl = WebSocketImpl;
    this.timeoutMs = timeoutMs; this.trace = trace; this.pending = new Map();
    this.listeners = new Set(); this.ws = null; this.authenticated = false;
  }

  async connect() {
    if (this.ws) throw new Error('Client already connected; close before reconnecting');
    const ws = new this.WebSocketImpl(this.url); this.ws = ws;
    ws.addEventListener('message', event => {
      if (this.ws !== ws) return;
      try {
        if (typeof event.data !== 'string') throw new Error('Expected text envelope');
        const frame = JSON.parse(event.data);
        if (frame.v !== 1 || typeof frame.type !== 'string' || typeof frame.id !== 'string') throw new Error('Malformed UCTP envelope');
        this.trace('recv', redact(frame));
        const pending = frame.in_reply_to && this.pending.get(frame.in_reply_to);
        if (pending) {
          clearTimeout(pending.timer); this.pending.delete(frame.in_reply_to);
          if (frame.type === 'error') pending.reject(new UctpError(frame.payload?.reason ?? 'UCTP error', { code: frame.payload?.code, request: pending.request, response: frame }));
          else pending.resolve(frame);
        } else {
          for (const listener of this.listeners) {
            try { listener(frame); } catch { /* An observer cannot break request correlation. */ }
          }
        }
      } catch {
        this.fail(new UctpError('Invalid UCTP frame')); ws.close(1002, 'Invalid frame');
      }
    });
    ws.addEventListener('close', () => {
      if (this.ws !== ws) return;
      this.ws = null; this.authenticated = false;
      this.fail(new UctpError('UCTP disconnected; operation outcome may be unknown'));
    });
    try {
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => { reject(new Error('UCTP connection timeout')); ws.close(); }, this.timeoutMs);
        const done = fn => { clearTimeout(timer); fn(); };
        ws.addEventListener('open', () => done(resolve), { once: true });
        ws.addEventListener('error', () => done(() => reject(new Error('UCTP connection failed'))), { once: true });
        ws.addEventListener('close', () => done(() => reject(new Error('UCTP connection closed'))), { once: true });
      });
      const hello = await this.request(envelope('auth.hello', {
        device: { id: `dev_${crypto.randomUUID()}`, kind: 'desktop', platform: 'javascript', sdk_version: 'parley-conference/1' },
        auth_methods: ['bearer'], capabilities: { application_profiles: [PROFILE] },
      }));
      if (hello.type !== 'auth.challenge' || !hello.payload?.server_capabilities?.application_profiles?.includes(PROFILE)) throw new Error('Server does not advertise conversation-control/1');
      const auth = await this.request(envelope('auth.response', { method: 'bearer', credential: this.token }, { in_reply_to: hello.id }));
      if (auth.type !== 'auth.session') throw new Error('Authentication did not establish a session');
      this.authenticated = true;
      this.identity = auth.payload.identity_id;
      return auth.payload;
    } catch (error) { this.close(); throw error; }
  }

  fail(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new UctpError(error.message, { request: pending.request, code: error.code }));
    }
    this.pending.clear();
  }

  close() {
    const ws = this.ws; this.ws = null; this.authenticated = false;
    this.fail(new UctpError('UCTP client closed'));
    ws?.close();
  }

  onEvent(listener) { this.listeners.add(listener); return () => this.listeners.delete(listener); }

  /** Persist mutating envelopes before sending. Retry the SAME id and payload. */
  request(request) {
    if (!this.ws || this.ws.readyState !== 1) return Promise.reject(new UctpError('UCTP is not connected', { request }));
    if (this.pending.has(request.id)) return Promise.reject(new UctpError('Request already pending', { request }));
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(request.id);
        reject(new UctpError('UCTP request timed out; outcome may be unknown', { request }));
      }, this.timeoutMs);
      this.pending.set(request.id, { resolve, reject, timer, request });
      try { this.trace('send', redact(request)); this.ws.send(JSON.stringify(request)); }
      catch (error) { clearTimeout(timer); this.pending.delete(request.id); reject(error); }
    });
  }

  command(type, cid, payload = {}, ids = {}) {
    if (!this.authenticated) throw new Error('Authenticate before sending commands');
    return envelope(type, { ...payload, profile: PROFILE }, { ...(cid ? { cid } : {}), ...ids });
  }

  async snapshot(cid, after = 0, live = false) {
    return (await this.request(this.command('conversation.subscribe', cid, { after, live }))).payload;
  }

  async history(cid) {
    const messages = []; let after = 0;
    for (;;) {
      const page = (await this.request(this.command('message.history', cid, { after }))).payload;
      messages.push(...page.messages);
      if (!page.has_more) return messages;
      if (!Number.isSafeInteger(page.cursor) || page.cursor <= after) throw new Error('History cursor did not advance');
      after = page.cursor;
    }
  }
}
