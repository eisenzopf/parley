// Explicit media gate, launched by the ignored Rust integration test.
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
const fixture = JSON.parse(process.env.MEDIA_FIXTURE);
const browser = await chromium.launch({
  channel: (process.env.CI || process.env.PARLEY_BROWSER_CHANNEL === 'chromium') ? 'chromium' : 'chrome',
  args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-device-for-media-stream'],
});
try {
  const page = await browser.newPage();
  page.on('console', message => console.error(message.text()));
  await page.goto(`${fixture.http}/conference/`);
  const permissions = await page.context().newCDPSession(page);
  const { targetInfo } = await permissions.send('Target.getTargetInfo');
  await permissions.send('Browser.setPermission', { permission: { name: 'microphone' },
    setting: 'denied', origin: fixture.http, browserContextId: targetInfo.browserContextId });
  const result = await page.evaluate(async f => {
    const { UctpClient } = await import('/uctp-client/client.mjs');
    const { BrowserAudio } = await import('/uctp-client/browser-audio.mjs');
    const sentCommands = [];
    const client = new UctpClient(f.url, f.token, { timeoutMs: 20000,
      trace: (direction, frame) => { if (!frame.type.startsWith('auth.')) { console.log(direction,frame.type,frame.payload?.kind || frame.payload?.reason || ''); if (direction==='send') sentCommands.push(frame); } },
    }); await client.connect();
    // Actual Chrome permission denial must fail before allocating any server connection.
    if ((await navigator.permissions.query({ name: 'microphone' })).state !== 'denied') throw new Error('Microphone permission was not denied by Chrome');
    const beforeDenied = await (await fetch('/__media_fixture')).json();
    let denied = false;
    try { await new BrowserAudio(client, f.cid, new Audio()).join(f.sid); }
    catch (error) { if (error.name === 'NotAllowedError') denied = true; else throw error; }
    if (!denied || sentCommands.length !== 0) throw new Error('Microphone denial dispatched a server operation');
    await new Promise(resolve=>setTimeout(resolve,400));
    const afterDenied = await (await fetch('/__media_fixture')).json();
    if (!afterDenied.original_bridge || afterDenied.ai_frames<=beforeDenied.ai_frames+5 || afterDenied.provider_frames<=beforeDenied.provider_frames+5) throw new Error('Microphone denial interrupted existing audio');
    // A browser that never answers SDP must not disrupt the existing audio.
    const before = afterDenied;
    const abandoned = await client.request(client.command('session.update',f.cid,{kind:'join_browser'},{sid:f.sid}));
    let rejected = false;
    try {
      await client.request(client.command('session.update',f.cid,{kind:'handoff_to_browser'},{sid:f.sid,connid:abandoned.connid}));
    } catch (error) { if (error.name === 'UctpError' && error.code) rejected=true; else throw error; }
    if (!rejected) throw new Error('Unconnected browser handoff unexpectedly succeeded');
    await client.request(client.command('connection.end',f.cid,{}, {sid:f.sid,connid:abandoned.connid}));
    await new Promise(resolve=>setTimeout(resolve,400));
    const recovered = await (await fetch('/__media_fixture')).json();
    if (!recovered.original_bridge || recovered.ai_frames<=before.ai_frames+5 || recovered.provider_frames<=before.provider_frames+5) {
      throw new Error(`Failed handoff interrupted the original bidirectional audio: ${JSON.stringify({before,recovered})}`);
    }
    // Use generated audio to prove the browser media path without a physical mic.
    const source = new AudioContext(); await source.resume();
    const oscillator = source.createOscillator(); oscillator.frequency.value = 880;
    const gain = source.createGain(); gain.gain.value = 0.3;
    const destination = source.createMediaStreamDestination();
    oscillator.connect(gain).connect(destination); oscillator.start();
    navigator.mediaDevices.getUserMedia = async () => destination.stream;
    const audio = new Audio(); audio.autoplay = true; document.body.append(audio);
    const controller = new BrowserAudio(client, f.cid, audio);
    const result = await controller.join(f.sid);
    const sink = new AudioContext(); await sink.resume();
    const analyzer = sink.createAnalyser(); analyzer.fftSize = 4096;
    sink.createMediaStreamSource(audio.srcObject).connect(analyzer);
    const bins = new Float32Array(analyzer.frequencyBinCount);
    let peak = 0; let audible = 0; let inbound = 0; let outbound = 0; let codec;
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      analyzer.getFloatFrequencyData(bins);
      let strongest = -Infinity;
      for (let i=1; i<bins.length; i++) if (bins[i]>strongest) {strongest=bins[i];peak=i*sink.sampleRate/analyzer.fftSize;}
      if (Math.abs(peak-660)<35 && strongest>-50) audible++;
      const stats = await controller.peer.getStats();
      for (const stat of stats.values()) {
        if (stat.type==='inbound-rtp' && stat.kind==='audio') inbound=stat.packetsReceived;
        if (stat.type==='outbound-rtp' && stat.kind==='audio') { outbound=stat.packetsSent; codec=stats.get(stat.codecId)?.mimeType; }
      }
      if (audible>=8 && inbound>20 && outbound>20) break;
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    // Allow the old jitter buffer to drain, then prove AI audio stays retired
    // while the browser continues speaking to the same telephone connection.
    await new Promise(resolve => setTimeout(resolve, 200));
    const retiredBefore = await (await fetch('/__media_fixture')).json();
    await new Promise(resolve => setTimeout(resolve, 400));
    const retiredAfter = await (await fetch('/__media_fixture')).json();
    if (retiredAfter.original_bridge || retiredAfter.ai_frames !== retiredBefore.ai_frames ||
        retiredAfter.provider_frames !== retiredBefore.provider_frames || retiredAfter.human_frames <= retiredBefore.human_frames + 5) {
      throw new Error(`AI audio was not retired while browser audio continued: ${JSON.stringify({retiredBefore,retiredAfter})}`);
    }
    // Closing Chrome follows test completion; Rust checks the retained SIP call
    // before the telephone participant hangs up.
    return { state: result.payload.session.state, retained: result.payload.session.retained_connid, rollbackVerified: true, microphoneDenialVerified: true, aiRetirementVerified: true, codec, peak, audible, inbound, outbound };
  }, fixture);
  assert.equal(result.state, 'speaking');
  assert.equal(result.codec?.toLowerCase(), 'audio/opus');
  assert.ok(result.audible >= 8, `SIP 660Hz audio not decoded in browser: ${JSON.stringify(result)}`);
  assert.ok(result.inbound > 20 && result.outbound > 20, JSON.stringify(result));
  console.log(JSON.stringify({ event: 'browser.media.proved', ...result }));
} finally { await browser.close(); }
