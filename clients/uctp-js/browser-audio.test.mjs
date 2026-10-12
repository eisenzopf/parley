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

test('changing microphone replaces the existing sender and only retires the old track after success', async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
  let stopped = 0, replacementStopped = 0, fail = false; const constraints = [], streams = [];
  Object.defineProperty(globalThis, 'navigator', { configurable: true, value: { mediaDevices: { getUserMedia: async value => {
    constraints.push(value); const track = { kind: 'audio', stop: () => replacementStopped++ };
    const stream = { getAudioTracks: () => [track], getTracks: () => [track] }; streams.push(stream); return stream;
  } } } });
  try {
    const updates = [], sender = { track: { kind: 'audio' }, replaceTrack: async track => {
      if (fail) throw new Error('replacement failed'); sender.track = track;
    } };
    const audio = new BrowserAudio({}, 'conversation', {}, { onStream: stream => updates.push(stream) });
    audio.peer = { getSenders: () => [sender] }; audio.media = { getTracks: () => [{ stop: () => stopped++ }] };
    await audio.setMicrophone('studio-display');
    assert.deepEqual(constraints[0].audio.deviceId, { exact: 'studio-display' });
    assert.equal(constraints[0].audio.echoCancellation, true); assert.equal(stopped, 1);
    assert.equal(audio.media, streams[0]); assert.equal(updates[0], streams[0]);
    audio.setMuted(true); assert.equal(audio.media.getAudioTracks()[0].enabled, false);
    await audio.setMicrophone('muted-headset');
    assert.equal(audio.media.getAudioTracks()[0].enabled, false, 'changing devices must preserve mute');
    audio.setMuted(false); assert.equal(audio.media.getAudioTracks()[0].enabled, true);
    const retained = audio.media;
    fail = true; await assert.rejects(audio.setMicrophone('headset'), /replacement failed/);
    assert.equal(audio.media, retained); assert.equal(replacementStopped, 2, 'old and failed replacement tracks are stopped');
    assert.equal(audio.microphoneId, 'muted-headset');
  } finally { if (original) Object.defineProperty(globalThis, 'navigator', original); else delete globalThis.navigator; }
});

test('speaker routing stores only a successful selection and defaults remain usable without setSinkId', async () => {
  const routed = [];
  const audio = new BrowserAudio({}, 'conversation', { setSinkId: async id => { if (id === 'missing') throw new Error('unavailable'); routed.push(id); } });
  await audio.setSpeaker('monitor-speakers'); await assert.rejects(audio.setSpeaker('missing'), /unavailable/);
  assert.deepEqual(routed, ['monitor-speakers']); assert.equal(audio.speakerId, 'monitor-speakers');
  const unsupported = new BrowserAudio({}, 'conversation', {});
  await unsupported.setSpeaker(''); await assert.rejects(unsupported.setSpeaker('headset'), /sound settings/);
});
