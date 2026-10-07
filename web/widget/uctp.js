/** UCTP/WS client — one JSON envelope per WebSocket text frame. */

/** rvoip WebRTC binds IPv4. Drop IPv6 ICE so the answerer never tries it. */
export function ipv4OnlySdp(sdp) {
  if (!sdp) return sdp;
  const nl = sdp.includes("\r\n") ? "\r\n" : "\n";
  return sdp
    .split(/\r?\n/)
    .filter((line) => {
      if (!/^a=candidate:/i.test(line)) return true;
      const addr = line.trim().split(/\s+/)[4] || "";
      return !addr.includes(":");
    })
    .map((line) => (/^c=IN IP6 /i.test(line) ? "c=IN IP4 0.0.0.0" : line))
    .join(nl);
}

export function envelope(type, payload, ids = {}) {
  return {
    v: 1,
    type,
    id: `env_${crypto.randomUUID().replace(/-/g, "")}`,
    ts: new Date().toISOString(),
    cid: ids.cid ?? undefined,
    sid: ids.sid ?? undefined,
    connid: ids.connid ?? undefined,
    in_reply_to: ids.in_reply_to ?? undefined,
    payload,
  };
}

export class UctpClient {
  constructor(url, token, trace) {
    this.url = url;
    this.token = token;
    this.trace = trace;
    this.ws = null;
    this.pending = [];
    this.byType = new Map();
  }

  connect() {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(this.url);
      this.ws = ws;
      const timer = setTimeout(() => {
        try { ws.close(); } catch (_) {}
        reject(new Error("uctp connect timeout"));
      }, 8000);
      ws.onopen = () => { clearTimeout(timer); resolve(); };
      ws.onerror = () => { clearTimeout(timer); reject(new Error("uctp connect failed")); };
      ws.onmessage = (ev) => {
        if (typeof ev.data !== "string") return;
        const env = JSON.parse(ev.data);
        if (this.trace) this.trace("recv", env);
        const typed = this.byType.get(env.type);
        if (typed && typed.length) {
          typed.shift()(env);
          return;
        }
        const waiter = this.pending.shift();
        if (waiter) waiter(env);
      };
    });
  }

  send(env) {
    if (!this.ws) throw new Error("not connected");
    if (this.trace) this.trace("send", env);
    this.ws.send(JSON.stringify(env));
  }

  next(ms = 8000) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("uctp wait timeout")), ms);
      this.pending.push((env) => {
        clearTimeout(timer);
        resolve(env);
      });
    });
  }

  waitType(type, ms = 15000) {
    return new Promise((resolve, reject) => {
      const q = this.byType.get(type) ?? [];
      const done = (env) => {
        clearTimeout(timer);
        resolve(env);
      };
      const timer = setTimeout(() => {
        const cur = this.byType.get(type) ?? [];
        this.byType.set(type, cur.filter((fn) => fn !== done));
        reject(new Error(type + " timeout"));
      }, ms);
      q.push(done);
      this.byType.set(type, q);
    });
  }

  async auth() {
    this.send(
      envelope("auth.hello", {
        device: {
          id: "dev_widget",
          kind: "browser",
          platform: "web",
          sdk_version: "parley-widget/0.1",
        },
        auth_methods: ["bearer"],
        capabilities: {},
      }),
    );
    const challenge = await this.next();
    this.send(
      envelope(
        "auth.response",
        { method: "bearer", credential: this.token, actor_token: null },
        { in_reply_to: challenge.id },
      ),
    );
    return this.next();
  }
}
