/** Human captions + redacted JSON for the voip-3 / UCTP inspector. */

export function captionFor(type) {
  return (
    {
      "conversation.create":
        "Opens a Conversation — the durable object. Chat, Talk, and SMS attach here.",
      "conversation.opened": "The server accepted the Conversation.",
      "session.invite":
        "Starts a Session inside that Conversation. Voice and live chat are Sessions, not new tickets.",
      "session.end": "Ends the Session. The Conversation stays open.",
      "message.send": "A Message on the Conversation. An SMS is the same noun.",
      "connection.offer":
        "A Connection into the Session. WebRTC is a Connection, not a UCTP tunnel.",
      "connection.answer": "The Connection is live.",
      "auth.hello": "UCTP authentication.",
      "auth.response": "UCTP authentication.",
      "auth.ok": "Authenticated.",
    }[type] || "UCTP envelope. The type is a voip-3 verb."
  );
}

export function publicEnvelope(env) {
  const copy = JSON.parse(JSON.stringify(env || {}));
  if (copy.payload && typeof copy.payload === "object") {
    if (copy.payload.credential) copy.payload.credential = "<redacted>";
    if (copy.payload.token) copy.payload.token = "<redacted>";
    const setup = copy.payload.substrate_setup;
    if (setup && typeof setup.sdp === "string" && setup.sdp.length > 180) {
      setup.sdp = setup.sdp.slice(0, 180) + "\n…";
    }
  }
  return copy;
}

export function attachTrace(listEl) {
  function add(kind, title, why, body) {
    if (!listEl) return;
    const empty = listEl.querySelector("[data-empty]");
    if (empty) empty.remove();
    const article = document.createElement("article");
    article.className = "trace";
    const k = document.createElement("div");
    k.className = "trace-k";
    k.textContent = kind;
    const h = document.createElement("h3");
    h.textContent = title;
    const p = document.createElement("p");
    p.textContent = why;
    const pre = document.createElement("pre");
    pre.textContent =
      typeof body === "string" ? body : JSON.stringify(body, null, 2);
    article.append(k, h, p, pre);
    listEl.prepend(article);
  }
  return {
    uctp(dir, env) {
      add(
        dir === "send" ? "UCTP →" : "UCTP ←",
        env.type || "envelope",
        captionFor(env.type),
        publicEnvelope(env),
      );
    },
    rest(method, path, req, res) {
      add(
        "HTTPS",
        method + " " + path,
        "Backends use the same nouns over HTTPS. Widgets and the desk should speak UCTP.",
        { request: req || null, response: res || null },
      );
    },
    record(title, why, body) {
      add("vCon", title, why, body);
    },
  };
}
