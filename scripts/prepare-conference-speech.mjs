// Optional macOS fixture generator. The output is synthetic speech, never a recording of a person.
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { resolve, join } from 'node:path';
if (process.platform !== 'darwin') throw new Error('Supply CONFERENCE_SPEECH_PCM: raw 8 kHz mono signed 16-bit little-endian synthetic speech. See the runbook.');
const mode = process.argv[2] || 'probe';
if (!['probe', 'scenario', 'pstn'].includes(mode)) throw new Error('Usage: prepare-conference-speech.mjs [probe|scenario|pstn]');
const output = resolve(mode === 'scenario' ? 'var/conference/scenario-speech' : mode === 'pstn' ? 'var/conference/pstn-speech' : 'var/conference/voice-probe');
const voiceScenario = mode === 'scenario' && process.env.CONFERENCE_DEMO_MODE === 'voice-only'
  ? JSON.parse(await readFile('config/conference-voice-scenario.json', 'utf8')) : null;
const texts = voiceScenario ? { booker: voiceScenario.booker_speech, organizer: voiceScenario.organizer_speech } : mode === 'scenario' ? {
  booker: 'Hello. I have a sandbox option for Jonathan and Alex. There are two seats on flight R D seven four two, departing at sixteen hundred and arriving at seventeen hundred at terminal C. No real booking has been made. Those are all the itinerary details.',
  organizer: 'Yes. I can arrange pickup for Jonathan and Alex at terminal C at seventeen hundred. Please bring Jonathan into this call to confirm the pickup.',
} : mode === 'pstn' ? {
  browser: 'This is the browser speaking through your existing telephone call. The confirmation code is orange bicycle. Please say orange bicycle back now, then tell us whether you heard this browser message clearly.',
} : { organizer: 'Yes. This is a sandbox test. Pickup is confirmed at terminal C at five p.m. The confirmation word is pineapple. Please repeat the confirmation word.' };
await mkdir(output, { recursive: true, mode: 0o700 });
const temp = await mkdtemp(join(output, 'speech-'));
try {
  for (const [role, text] of Object.entries(texts)) {
    execFileSync('/usr/bin/say', ['-r', '145', '-o', join(temp, 'organizer.aiff'), text]);
    execFileSync('/usr/bin/afconvert', ['-f', 'WAVE', '-d', 'LEI16@8000', '-c', '1', join(temp, 'organizer.aiff'), join(temp, 'organizer.wav')]);
    const wav = await readFile(join(temp, 'organizer.wav'));
    if (wav.toString('ascii', 0, 4) !== 'RIFF' || wav.toString('ascii', 8, 12) !== 'WAVE') throw new Error('Expected a WAV speech fixture');
    let formatOk = false, pcm;
    for (let offset = 12; offset + 8 <= wav.length;) {
      const type = wav.toString('ascii', offset, offset + 4), size = wav.readUInt32LE(offset + 4), start = offset + 8;
      if (start + size > wav.length) throw new Error('Truncated WAV fixture');
      if (type === 'fmt ' && size >= 16) formatOk = wav.readUInt16LE(start) === 1 && wav.readUInt16LE(start + 2) === 1
        && wav.readUInt32LE(start + 4) === 8000 && wav.readUInt16LE(start + 14) === 16;
      if (type === 'data') pcm = wav.subarray(start, start + size);
      offset = start + size + (size % 2);
    }
    if (!formatOk || !pcm || pcm.length % 2 || pcm.length <= 16000 || pcm.length > 720000) throw new Error('Expected 1–45 seconds of 8 kHz mono PCM speech');
    await writeFile(join(output, `${role}.pcm`), pcm, { mode: 0o600 });
    console.log(JSON.stringify({ role, synthetic_speech_seconds: pcm.length / 16000, path: join(output, `${role}.pcm`) }));
    await rm(join(temp, 'organizer.aiff')); await rm(join(temp, 'organizer.wav'));
  }
} finally { await rm(temp, { recursive: true, force: true }); }
