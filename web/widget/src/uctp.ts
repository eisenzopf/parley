/** UCTP/WS client — one JSON envelope per WebSocket text frame. */

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
  constructor(url, token) {
    this.url = url;
    this.token = token;
    this.ws = null;
    this.pending = [];
    this.byType = new Map();
  }

  connect() {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(this.url);
      this.ws = ws;
      ws.onopen = () => resolve();
      ws.onerror = (err) => reject(err);
      ws.onmessage = (ev) => {
        if (typeof ev.data !== "string") return;
        const env = JSON.parse(ev.data);
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
    this.ws.send(JSON.stringify(env));
  }

  next() {
    return new Promise((resolve) => this.pending.push(resolve));
  }

  waitType(type) {
    return new Promise((resolve) => {
      const q = this.byType.get(type) ?? [];
      q.push(resolve);
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
