import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, stat, symlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectEvidence } from './evidence.mjs';
import { saveEvidence } from '../../scripts/export-conference-evidence.mjs';

const cid = 'conv_export';
const event = (seq, request_id = 'env_send') => ({ cid, seq, request_id, event_type: 'message.accepted', payload: { body: 'Sandbox message', token: 'PRIVATE_EVENT' } });
function fixture(pages = [[event(1)]]) {
  const reads = [], inspections = [];
  return { identity: 'participant:owner', reads, inspections,
    snapshot: async (requestedCid, after) => {
      assert.equal(requestedCid, cid); reads.push(after);
      return { state: 'open', participants: [{ subject: 'participant:owner', participant_id: 'part_owner', name: 'Owner', role: 'owner', sms: 'PRIVATE_PHONE', sip: 'PRIVATE_ROUTE' }],
        capabilities: { sms_mode: 'fake', implementation: { control_transport: 'websocket' } }, events: pages[reads.length - 1] || [] };
    },
    command: (type, requestedCid, payload) => {
      assert.equal(type, 'conversation.inspect'); assert.equal(requestedCid, cid); return { type, payload };
    },
    request: async ({ payload }) => {
      inspections.push(payload.request_id);
      if (payload.request_id === 'external') throw Object.assign(new Error('No command'), { code: 404 });
      return { payload: { evidence: { actor_subject: 'participant:owner',
        request: { type: 'message.send', id: payload.request_id, cid, signature: 'PRIVATE_SIGNATURE', payload: { body: 'Sandbox message', credential: 'PRIVATE_REQUEST' } },
        response: { type: 'ack', cid, in_reply_to: payload.request_id, payload: { sdp: 'a=ice-pwd:PRIVATE_SDP\r\na=ice-ufrag:PRIVATE_UFRAG\r\n', token: 'PRIVATE_RESPONSE' } },
      } } };
    },
  };
}

test('evidence drains journal pages with sequence gaps and preserves correlated outcomes without credentials', async () => {
  const client = fixture([Array.from({ length: 500 }, (_, i) => event(i * 2 + 1)), [event(1005, 'external')]]);
  const output = await collectEvidence(client, cid, { mode: 'local-fixture' });
  assert.deepEqual(client.reads, [0, 999]);
  assert.deepEqual(client.inspections, ['env_send', 'external']);
  assert.equal(output.events.length, 501);
  assert.equal(output.commands.length, 1);
  assert.equal(output.commands[0].event_seqs.length, 500);
  assert.equal(output.commands[0].request.payload.body, 'Sandbox message');
  assert.equal(output.commands[0].response.in_reply_to, 'env_send');
  assert.deepEqual(output.unavailable, [{ request_id: 'external', code: 404 }]);
  assert.equal(output.capture.through_seq, 1005);
  assert.equal(output.capture.atomic_snapshot, false);
  assert.ok(!JSON.stringify(output).includes('PRIVATE_'));
});

test('evidence refuses incorrect correlation, foreign events, stalled pages, missing scope and false provider labels', async () => {
  for (const events of [[{ ...event(1), cid: 'conv_other' }], [event(1), event(1)]]) {
    await assert.rejects(collectEvidence(fixture([events]), cid, { mode: 'local-fixture' }), /Foreign or nonadvancing/);
  }
  await assert.rejects(collectEvidence(fixture([Array.from({ length: 500 }, (_, i) => event(i + 1))]), cid,
    { mode: 'local-fixture', maxPages: 1 }), /page limit reached/);
  await assert.rejects(collectEvidence(fixture(), cid, { mode: 'live-providers' }), /SMS mode/);
  const stranger = fixture(); stranger.identity = 'participant:stranger';
  await assert.rejects(collectEvidence(stranger, cid, { mode: 'local-fixture' }), /scope required/);
  const wrong = fixture(); const original = wrong.request;
  wrong.request = async command => { const result = await original(command); result.payload.evidence.response.in_reply_to = 'env_other'; return result; };
  await assert.rejects(collectEvidence(wrong, cid, { mode: 'local-fixture' }), /Mismatched/);
  const disconnected = fixture(); disconnected.request = async () => { throw new Error('disconnected'); };
  await assert.rejects(collectEvidence(disconnected, cid, { mode: 'local-fixture' }), /disconnected/);
});

test('evidence files are private and cannot overwrite an existing run or symlink target', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'parley-evidence-'));
  try {
    const path = join(directory, 'run.json'), link = join(directory, 'link.json');
    await saveEvidence(path, { cid });
    assert.equal((await stat(path)).mode & 0o777, 0o600);
    await assert.rejects(saveEvidence(path, { cid: 'replacement' }), { code: 'EEXIST' });
    await symlink(path, link);
    await assert.rejects(saveEvidence(link, { cid: 'replacement' }), { code: 'EEXIST' });
    assert.deepEqual(JSON.parse(await readFile(path, 'utf8')), { cid });
  } finally { await rm(directory, { recursive: true, force: true }); }
});
