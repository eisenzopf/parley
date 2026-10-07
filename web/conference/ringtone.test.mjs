import test from 'node:test';
import assert from 'node:assert/strict';
import { BrowserRingtone } from './ringtone.mjs';

test('one live invitation rings in bursts, stops on answer and cannot ring again after polling', async () => {
  const frequencies = []; let ticks, cancelled = 0, stops = 0;
  class Audio {
    state = 'suspended'; currentTime = 10; destination = {};
    async resume() { this.state = 'running'; }
    createGain() { return { gain: {}, connect() {}, disconnect() {} }; }
    createOscillator() { return { frequency: {}, connect() {}, disconnect() {},
      start() { frequencies.push(this.frequency.value); }, stop() { stops++; } }; }
  }
  const ring = new BrowserRingtone({ AudioContextImpl: Audio,
    schedule: callback => { ticks = callback; return 1; }, cancel: () => cancelled++ });
  await ring.unlock(); ring.start('organizer-session'); ring.start('organizer-session');
  assert.deepEqual(frequencies, [440, 480]);
  ticks(); assert.deepEqual(frequencies, [440, 480, 440, 480]);
  ring.answer('organizer-session'); const before = frequencies.length;
  ring.start('organizer-session'); ticks();
  assert.equal(frequencies.length, before); assert.equal(ring.sid, null);
  assert.equal(cancelled, 1); assert.ok(stops >= 4);
  ring.start('next-session'); assert.equal(frequencies.length, before + 2);
  ring.start(null); assert.equal(ring.sid, null); assert.equal(cancelled, 2);
});
