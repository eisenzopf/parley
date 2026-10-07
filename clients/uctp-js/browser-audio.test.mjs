import test from 'node:test';
import assert from 'node:assert/strict';
import { BrowserAudio } from './browser-audio.mjs';

test('ambiguous callback request replays the exact envelope and cannot supply a dial address', async () => {
  const requests = []; let next = 0;
  const client = {
    command: (type, cid, payload, ids) => ({ id: `request-${++next}`, type, cid, payload, ...ids }),
    request: async request => { requests.push(request); if (requests.length === 1) throw new Error('timeout'); return { payload: { session: { sid: 'session', connid: 'phone' } } }; },
  };
  const audio = new BrowserAudio(client, 'conversation', {});
  audio.peer = {}; audio.connid = 'browser'; audio.sid = 'session';
  await assert.rejects(audio.moveToPhone(), /timeout/);
  await audio.moveToPhone();
  assert.strictEqual(requests[0], requests[1]);
  assert.deepEqual(requests[1].payload, { kind: 'move_to_phone' });
  assert.equal(requests[1].connid, 'browser');
  await audio.cancelPhoneMove();
  assert.equal(requests[2].connid, 'phone');
  assert.deepEqual(requests[2].payload, { kind: 'cancel_phone_move' });
  audio.resetPhoneMove(); await audio.moveToPhone();
  assert.notEqual(requests[3].id, requests[0].id);
});

test('a phone move requires browser audio and unknown callbacks cannot be blindly cancelled', async () => {
  const audio = new BrowserAudio({}, 'conversation', {});
  await assert.rejects(audio.moveToPhone(), /Join through the browser/);
  await assert.rejects(audio.cancelPhoneMove(), /inspect its journal outcome/);
});
