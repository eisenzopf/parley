// Live Vapi is attached by the Rust gate; browser microphone and SIP audio are synthetic.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { chromium, expect } from '@playwright/test';
const fixture = JSON.parse(process.env.LIVE_VOICE_FIXTURE);
const { cloudProbe, sipProbe } = await import('./support/cloud-probe.mjs');
const browser = await chromium.launch({ channel: (process.env.CI || process.env.PARLEY_BROWSER_CHANNEL === 'chromium') ? 'chromium' : 'chrome',
  args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
});
let page;
try {
  page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
  await page.goto(`${fixture.http}/conference/`);
  await page.evaluate(() => {
    const label = document.createElement('p');
    label.textContent = 'AWS NETWORK TEST · Real WebRTC from this Mac; synthetic SIP audio on AWS. No PSTN or SMS.';
    label.style.cssText = 'margin:4px 0 0;color:#f6c986;font-size:10px'; document.querySelector('header > div').append(label);
  });
  await page.locator('#url').fill(fixture.url); await page.locator('#cid').fill(fixture.cid); await page.locator('#token').fill(fixture.token);
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.locator('#join')).toBeEnabled();
  await expect(page.locator('[data-edge=vapi]')).toHaveAttribute('data-state', 'active');
  await page.evaluate(async () => {
    const NativePeer = window.RTCPeerConnection;
    window.RTCPeerConnection = class extends NativePeer { constructor(...args) {
      super(...args); window.probePeer = this;
      this.addEventListener('connectionstatechange', () => {
        if (this.connectionState === 'connected' && !window.probeConnectedAt) window.probeConnectedAt = Date.now();
      });
    } };
    const source = new AudioContext(); await source.resume();
    const oscillator = source.createOscillator(); oscillator.frequency.value = 880;
    const gain = source.createGain(); gain.gain.value = 0.3;
    const destination = source.createMediaStreamDestination(); oscillator.connect(gain).connect(destination); oscillator.start();
    window.probeSource = source;
    navigator.mediaDevices.getUserMedia = async () => destination.stream;
    const sink = new AudioContext(); await sink.resume(); let analyzer, bins;
    window.probeAudioTimer = setInterval(() => {
      const stream = document.getElementById('remote-audio').srcObject;
      if (!stream) return;
      if (!analyzer) {
        analyzer = sink.createAnalyser(); analyzer.fftSize = 4096;
        bins = new Float32Array(analyzer.frequencyBinCount); sink.createMediaStreamSource(stream).connect(analyzer);
      }
      analyzer.getFloatFrequencyData(bins); let strongest = -Infinity, peak = 0;
      for (let i = 1; i < bins.length; i++) if (bins[i] > strongest) { strongest = bins[i]; peak = i * sink.sampleRate / analyzer.fftSize; }
      if (!window.probeAudibleAt && Math.abs(peak - 660) < 35 && strongest > -50) window.probeAudibleAt = Date.now();
    }, 10);
  });
  const baseline = await cloudProbe(fixture);
  assert.equal(baseline.sip_ended, false);
  const start = Date.now();
  await page.locator('#join').click();
  await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state', 'speaking', { timeout: 20000 });
  const handoffMs = Date.now() - start;
  await expect(page.locator('[data-edge=vapi]')).toHaveAttribute('data-state', 'retired');
  await expect(page.locator('#retained')).toContainText('Same telephone Connection retained');
  await expect(page.locator('#network-ids')).toContainText(fixture.connid);
  await expect(page.locator('#network-ids')).toContainText(fixture.sid);
  const measured = await page.evaluate(async () => {
    const sink = new AudioContext(); await sink.resume(); const analyzer = sink.createAnalyser(); analyzer.fftSize = 4096;
    sink.createMediaStreamSource(document.getElementById('remote-audio').srcObject).connect(analyzer);
    const bins = new Float32Array(analyzer.frequencyBinCount); let audible = 0, peak = 0, inbound = 0, outbound = 0, codec;
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      analyzer.getFloatFrequencyData(bins); let strongest = -Infinity;
      for (let i = 1; i < bins.length; i++) if (bins[i] > strongest) { strongest = bins[i]; peak = i * sink.sampleRate / analyzer.fftSize; }
      if (Math.abs(peak - 660) < 35 && strongest > -50) audible++;
      const stats = await window.probePeer.getStats();
      for (const stat of stats.values()) {
        if (stat.type === 'inbound-rtp' && stat.kind === 'audio') inbound = stat.packetsReceived;
        if (stat.type === 'outbound-rtp' && stat.kind === 'audio') { outbound = stat.packetsSent; codec = stats.get(stat.codecId)?.mimeType; }
      }
      if (audible >= 8 && inbound > 20 && outbound > 20) break;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    const stats = await window.probePeer.getStats();
    const transport = [...stats.values()].find(s => s.type === 'transport' && s.selectedCandidatePairId);
    const pair = stats.get(transport?.selectedCandidatePairId);
    const remote = stats.get(pair?.remoteCandidateId);
    return { audible, peak, inbound, outbound, codec, connection: window.probePeer.connectionState,
      ice: window.probePeer.iceConnectionState, dtls: transport?.dtlsState,
      remote: remote && {address: remote.address, port: remote.port, protocol: remote.protocol, candidateType: remote.candidateType} };
  });
  assert.ok(measured.audible >= 8 && measured.inbound > 20 && measured.outbound > 20, JSON.stringify(measured));
  assert.equal(measured.codec?.toLowerCase(), 'audio/opus');
  assert.equal(measured.remote?.address, fixture.publicIp);
  assert.equal(measured.remote?.protocol, 'udp');
  assert.equal(measured.dtls, 'connected');
  await writeFile(`${fixture.output}/audio-result.json`, JSON.stringify(measured,null,2), {mode:0o600});
  let state;
  const deadline = Date.now() + 30000;
  do {
    state = await cloudProbe(fixture);
    if (state.browser_audio_frames - baseline.browser_audio_frames > 10) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  } while (Date.now() < deadline);
  await writeFile(`${fixture.output}/sip-probe.json`, JSON.stringify(state,null,2), {mode:0o600});
  assert.equal(state.sip_ended, false, 'SIP call must remain alive after Vapi retires');
  assert.ok(state.browser_audio_frames - baseline.browser_audio_frames > 10, 'SIP must hear the browser tone');
  const timing = await page.evaluate(() => ({ connected_at: window.probeConnectedAt, browser_audio_at: window.probeAudibleAt }));
  assert.ok(timing.connected_at && timing.browser_audio_at && state.browser_first_audio_ms);
  const readyToAudioMs = Math.max(timing.browser_audio_at, state.browser_first_audio_ms) - timing.connected_at;
  assert.ok(readyToAudioMs >= 0 && readyToAudioMs <= 10000, `AWS media-ready to audible two-way audio: ${readyToAudioMs} ms`);
  await mkdir('test-results', { recursive: true });
  await page.screenshot({ path: `${fixture.output}/handoff.png`, fullPage: true });
  console.log(JSON.stringify({ event: 'cloud.webrtc.browser_handoff', handoff_ui_ms: handoffMs,
    ready_to_bidirectional_audio_ms: readyToAudioMs, audio: measured,
    browser_audio_frames: state.browser_audio_frames, sip_ended: state.sip_ended }));
  await writeFile(`${fixture.output}/browser-result.json`, JSON.stringify({measured, state, timing, handoffMs, readyToAudioMs}, null, 2), {mode:0o600});
  await page.locator('#end').click();
  await expect.poll(async () => (await sipProbe(fixture)).state === 'ended',
    { timeout: 20000 }).toBe(true);
} catch(error) {
  if (page) {
    await page.screenshot({path:`${fixture.output}/failure.png`, fullPage:true}).catch(() => {});
    const diagnostic = await page.evaluate(async () => ({notice:document.getElementById('notice')?.textContent,
      connection:window.probePeer?.connectionState, ice:window.probePeer?.iceConnectionState,
      stats:window.probePeer ? [...(await window.probePeer.getStats()).values()].filter(s => ['transport','candidate-pair','remote-candidate','inbound-rtp','outbound-rtp'].includes(s.type)) : []})).catch(() => ({}));
    await writeFile(`${fixture.output}/failure.json`, JSON.stringify({error:error.message, diagnostic},null,2), {mode:0o600});
    console.error(JSON.stringify({notice:diagnostic.notice, connection:diagnostic.connection, ice:diagnostic.ice}));
  }
  throw error;
} finally { await browser.close(); }
