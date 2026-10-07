import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { VapiPlanner } from './vapi.mjs';

test('provider errors and malformed decisions fail without issuing communications', async () => {
  let requests = 0;
  const planner = new VapiPlanner({ apiKey: 'fixture', fetchImpl: async (url, options) => {
    requests++;
    assert.equal(url, 'https://api.vapi.ai/chat');
    assert.equal(options.redirect, 'error');
    assert.deepEqual(JSON.parse(options.body).assistant.model.tools, []);
    assert.equal(JSON.parse(options.body).assistant.model.maxTokens, 4000);
    assert.equal(JSON.parse(options.body).assistant.model.model, 'gpt-4.1');
    return requests === 1 ? new Response('unavailable', { status: 503 })
      : Response.json({ output: [{ role: 'assistant', content: 'I sent all the messages!' }] });
  } });
  await assert.rejects(planner.decide({}), /503/);
  await assert.rejects(planner.decide({}), /must be JSON/);
  assert.equal(requests, 2);
});

test('live decision framing preserves JSON and rejects stripped or ambiguous responses', async () => {
  const responses = [
    '[\n  {\n    "actions": [{"type":"call_participant","to":"booker","purpose":"Find an itinerary"}],\n    "waiting_for": "booker"\n  }\n]',
    '"actions":[]}', '[{"actions":[]},{"actions":[]}]', 'null', '[]', '[[{"actions":[]}]]', '{}',
  ];
  const planner = new VapiPlanner({ apiKey: 'fixture', fetchImpl: async () => Response.json({
    id: 'fixture-chat', output: [{ role: 'assistant', content: responses.shift() }],
  }) });
  const decision = await planner.decide({});
  assert.equal(decision.actions[0].type, 'call_participant');
  assert.equal(decision.provider_chat_id, 'fixture-chat');
  while (responses.length) await assert.rejects(planner.decide({}), /no actions executed/);
});

test('credentials cannot be sent to arbitrary planner endpoints', () => {
  assert.throws(() => new VapiPlanner({ apiKey: 'fixture', endpoint: 'https://example.com/chat' }), /official API/);
});

test('header and response-body failures expose only sanitized retry diagnostics', async () => {
  for (const phase of ['headers', 'body']) {
    const planner = new VapiPlanner({ apiKey: 'private-fixture', fetchImpl: async (_url, options) => {
      assert.ok(options.signal instanceof AbortSignal);
      const timeout = new DOMException('private-provider-detail', 'TimeoutError');
      if (phase === 'headers') throw timeout;
      return { ok: true, json: async () => { throw timeout; } };
    } });
    await assert.rejects(planner.decide({}), error => {
      assert.equal(error.retryable, true);
      assert.equal(error.transportReason, 'timeout');
      assert.ok(Number.isInteger(error.elapsedMs));
      assert.equal(error.message, 'Vapi planning transport failed; no actions executed');
      return true;
    });
    assert.equal(planner.requestTimeoutMs, 20000);
  }
  const malformed = new VapiPlanner({ apiKey: 'fixture', fetchImpl: async () => new Response('{') });
  await assert.rejects(malformed.decide({}), error => !error.retryable && /Invalid Vapi response JSON/.test(error.message));
});

test('a stalled real HTTP response body obeys the planning deadline', async () => {
  let headersSent = false;
  const server = createServer((_request, response) => {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.flushHeaders(); headersSent = true;
    response.write('{'); // Deliberately leave the provider body unfinished.
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  try {
    const planner = new VapiPlanner({ apiKey: 'fixture', requestTimeoutMs: 200,
      endpoint: `http://127.0.0.1:${server.address().port}/chat` });
    await assert.rejects(planner.decide({}), error => error.retryable && error.transportReason === 'timeout');
    assert.equal(headersSent, true, 'the deadline must cover body consumption after headers arrive');
  } finally {
    server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  }
});
