// Invoked by the Rust test against a real local Parley UCTP host and fake SMS.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { UctpClient } from '../../clients/uctp-js/client.mjs';
import { ConferenceWorker, FileState } from './worker.mjs';
import { VapiPlanner } from './vapi.mjs';

const fixture = JSON.parse(await readFile(process.argv[2], 'utf8'));
let calls = 0;
const mockVapi = createServer(async (req, res) => {
  assert.equal(req.url, '/chat'); assert.equal(req.headers.authorization, 'Bearer test-vapi-key');
  let raw = ''; for await (const part of req) raw += part;
  const request = JSON.parse(raw); const context = JSON.parse(request.input);
  assert.equal(context.cid, fixture.cid); assert.deepEqual(request.assistant.model.tools, []);
  assert.ok(!raw.includes('test-admin')); assert.ok(!raw.includes(fixture.assistant_token));
  calls++;
  const actions = context.approval
    ? [{ type: 'final_updates', proposal_id: context.approval.proposal_id, updates: context.participants.filter(m => m.role !== 'assistant').map(m => ({ to: m.participant_id, body: `Your arrangements, ${m.name}: terminal C at 5pm.` })) }]
    : [{ type: 'propose_arrangements', summary: 'Sandbox itinerary: terminal C at 5pm. No real booking made.' }];
  res.writeHead(200, { 'content-type': 'application/json' });
  res.end(JSON.stringify({ id: `chat_fixture_${calls}`, output: [{ role: 'assistant', content: JSON.stringify({ actions }) }] }));
});
mockVapi.listen(0, '127.0.0.1'); await once(mockVapi, 'listening');
const trace = [];
const assistant = new UctpClient(fixture.url, fixture.assistant_token, { trace: (direction, frame) => trace.push({ direction, frame }) });
const owner = new UctpClient(fixture.url, fixture.owner_token);
try {
  await assistant.connect(); await owner.connect();
  const planner = new VapiPlanner({ apiKey: 'test-vapi-key', endpoint: `http://127.0.0.1:${mockVapi.address().port}/chat` });
  const storage = new FileState(fixture.state_path);
  let worker = new ConferenceWorker({ client: assistant, cid: fixture.cid, planner, storage });
  await owner.request(owner.command('message.send', fixture.cid, { msg_id: 'msg_task', to: [fixture.assistant_id], delivery: 'chat', body: 'My flight was canceled. Coordinate new arrangements with everyone.' }));
  await worker.step();
  assert.equal(calls, 1);
  assert.equal(worker.state.approved, null);
  assert.throws(() => worker.prepare({ actions: [{ type: 'final_updates', proposal_id: worker.state.proposal.id, updates: [] }] }), /approved/);
  // Pretending to approve as the AI must not unlock final notifications.
  await assistant.request(assistant.command('message.send', fixture.cid, { msg_id: 'msg_fake_approval', to: [fixture.owner_id], content_type: 'application/json', body: JSON.stringify({ type: 'travel.approval', version: 1, approved: true, proposal_id: worker.state.proposal.id }) }));
  await worker.step(); assert.equal(worker.state.approved, null); assert.equal(calls, 1);
  await owner.request(owner.command('message.send', fixture.cid, { msg_id: 'msg_approval', to: [fixture.assistant_id], content_type: 'application/json', body: JSON.stringify({ type: 'travel.approval', version: 1, approved: true, proposal_id: worker.state.proposal.id }) }));
  // Lose the first final-message ACK after the actual server accepted it.
  const request = assistant.request.bind(assistant); let loseAck = true;
  assistant.request = async frame => {
    const response = await request(frame);
    if (loseAck && frame.type === 'message.send') { loseAck = false; throw new Error('simulated ACK loss'); }
    return response;
  };
  await assert.rejects(worker.step(), /ACK loss/);
  assert.ok((await storage.load(fixture.cid)).pending);
  assistant.close(); await assistant.connect(); assistant.request = request;
  worker = new ConferenceWorker({ client: assistant, cid: fixture.cid, planner, storage });
  await worker.step(); await worker.step();
  assert.equal(calls, 2, 'restart replays saved effects without re-planning');
  assert.equal(worker.state.completedProposal, worker.state.approved.proposal_id);
  const history = await assistant.history(fixture.cid);
  const sms = history.filter(m => m.medium === 'sms');
  assert.equal(sms.length, 4); assert.equal(new Set(sms.map(m => m.id)).size, 4);
  assert.ok(sms.every(m => m.body.startsWith('[Sandbox arrangements]')));
  assert.ok(trace.filter(t => t.frame.type === 'message.send').every(t => t.frame.cid === fixture.cid));
  assert.ok(!JSON.stringify(trace).includes(fixture.assistant_token));
  // With UCTP disconnected, no provider-side fallback is invoked.
  assistant.close(); await assert.rejects(worker.step(), /Authenticate|connected/);
  assert.equal(calls, 2);
  console.log('external-worker-ok: Vapi fixture -> UCTP -> four SMS outbox records; ACK-loss replay; approval attribution; no fallback');
} finally { assistant.close(); owner.close(); mockVapi.close(); }
