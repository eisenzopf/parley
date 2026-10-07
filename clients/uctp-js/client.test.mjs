import test from 'node:test';
import assert from 'node:assert/strict';
import { UctpClient, envelope, redact } from './client.mjs';

test('TURN credentials reach the client but never the inspector trace', () => {
  const offer=envelope('connection.offer',{ice_servers:[{urls:['turn:relay.invalid:3478'],username:'scoped-client',credential:'PRIVATE_TURN'}]});
  const evidence=redact(offer);
  assert.equal(offer.payload.ice_servers[0].credential,'PRIVATE_TURN');
  assert.equal(evidence.payload.ice_servers[0].credential,'[redacted]');
  assert.deepEqual(evidence.payload.ice_servers[0].urls,offer.payload.ice_servers[0].urls);
});

class Socket extends EventTarget {
  constructor() { super(); this.readyState = 0; Socket.last = this; queueMicrotask(() => { this.readyState = 1; this.dispatchEvent(new Event('open')); }); }
  emit(frame) { this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(frame) })); }
  send(raw) {
    const frame = JSON.parse(raw);
    if (frame.type === 'auth.hello') queueMicrotask(() => this.emit(envelope('auth.challenge', { server_capabilities: { application_profiles: ['conversation-control/1'] } }, { in_reply_to: frame.id })));
    if (frame.type === 'auth.response') queueMicrotask(() => this.emit(envelope('auth.session', { identity_id: 'participant:ai', session_token: 'SECRET_SESSION' }, { in_reply_to: frame.id })));
  }
  close() { this.readyState = 3; this.dispatchEvent(new Event('close')); }
}

test('correlation survives reversed replies and unrelated events; auth is redacted', async () => {
  const trace = []; const events = [];
  const client = new UctpClient('ws://127.0.0.1:1', 'SECRET_BEARER', { WebSocketImpl: Socket, trace: (dir, frame) => trace.push({ dir, frame }) });
  await client.connect(); client.onEvent(frame => events.push(frame));
  const a = client.command('message.send', 'conv_a'); const b = client.command('message.send', 'conv_a');
  const first = client.request(a); const second = client.request(b);
  Socket.last.emit(envelope('conversation.event', { seq: 1 }));
  Socket.last.emit(envelope('ack', { name: 'second' }, { in_reply_to: b.id }));
  Socket.last.emit(envelope('ack', { name: 'first' }, { in_reply_to: a.id }));
  assert.equal((await first).payload.name, 'first'); assert.equal((await second).payload.name, 'second');
  assert.equal(events.length, 1); assert.equal(client.pending.size, 0);
  assert.ok(!JSON.stringify(trace).includes('SECRET_')); client.close();
});

test('timeout removes waiter; disconnect rejects outstanding commands with original envelope', async () => {
  const client = new UctpClient('ws://localhost:1', 'token', { WebSocketImpl: Socket, timeoutMs: 30 });
  await client.connect();
  const request = client.command('message.send', 'conv_a');
  await assert.rejects(client.request(request), error => error.request.id === request.id && /timed out/.test(error.message));
  assert.equal(client.pending.size, 0);
  const pending = client.request(request);
  Socket.last.close();
  await assert.rejects(pending, error => error.request.id === request.id && /disconnected/.test(error.message));
  assert.equal(client.pending.size, 0); assert.equal(client.authenticated, false);
});

test('remote cleartext and URL credentials are rejected', () => {
  assert.throws(() => new UctpClient('ws://example.com', 'token'), /wss/);
  assert.throws(() => new UctpClient('wss://token@example.com', 'token'), /URL/);
});

test('history drains bounded pages and rejects a stalled cursor', async () => {
  const client = new UctpClient('ws://localhost:1', 'token');
  client.authenticated = true;
  const requested = []; let stalled = false;
  client.request = async frame => {
    requested.push(frame);
    return { payload: frame.payload.after === 0
      ? { messages: [{ id: 'msg_first' }], cursor: 501, has_more: true }
      : { messages: [{ id: 'msg_second' }], cursor: 501, has_more: stalled } };
  };
  assert.deepEqual((await client.history('conv_pages')).map(m => m.id), ['msg_first', 'msg_second']);
  assert.deepEqual(requested.map(f => f.payload.after), [0, 501]);
  assert.notEqual(requested[0].id, requested[1].id);
  stalled = true;
  await assert.rejects(client.history('conv_pages'), /did not advance/);
});


test('inspector scrubs SDP authentication and key material without changing negotiation', () => {
  const sdp='v=0\r\na=ice-ufrag:PRIVATE_USER\r\na=ice-pwd:PRIVATE_ICE\r\na=crypto:1 AES_CM_128_HMAC_SHA1_80 inline:PRIVATE_KEY\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n';
  const offer=envelope('connection.offer',{substrate_setup:{sdp}});
  const safe=redact(offer);
  assert.equal(offer.payload.substrate_setup.sdp,sdp);
  assert.ok(!JSON.stringify(safe).includes('PRIVATE_'));
  assert.ok(safe.payload.substrate_setup.sdp.includes('m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n'));
});
