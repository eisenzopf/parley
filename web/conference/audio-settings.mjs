import { openMicrophone } from '/uctp-client/browser-audio.mjs';

// Device preferences stay in this browser. A microphone check never monitors
// its input through speakers or records audio, and opens no call connection.
export class AudioSettings {
  constructor({ root, remoteAudio, setMicrophone, setSpeaker }) {
    this.root = root; this.remoteAudio = remoteAudio;
    this.setMicrophone = setMicrophone; this.setSpeaker = setSpeaker;
    this.mic = root.querySelector('#microphone'); this.speaker = root.querySelector('#speaker');
    this.check = root.querySelector('#check-mic'); this.meter = root.querySelector('#mic-level');
    this.label = root.querySelector('#mic-status'); this.status = root.querySelector('#audio-status');
    this.preferences = { microphoneId: '', speakerId: '' };
    try { Object.assign(this.preferences, JSON.parse(localStorage.getItem('parley.audio') || '{}')); } catch {}
    this.mic.onchange = async () => {
      const previous = this.preferences.microphoneId;
      this.mic.disabled = true;
      try {
        this.stopCheck(); await this.setMicrophone(this.mic.value);
        this.preferences.microphoneId = this.mic.value; this.save();
        this.status.textContent = this.callStream ? 'Microphone changed for this call.' : 'Microphone selected. Use Check microphone to verify it.';
      } catch (error) { this.mic.value = previous; this.report(error); }
      finally { this.mic.disabled = false; }
    };
    this.speaker.onchange = async () => {
      const previous = this.preferences.speakerId;
      try { await this.setSpeaker(this.speaker.value); this.preferences.speakerId = this.speaker.value; this.save(); this.status.textContent = 'Speaker selected.'; }
      catch (error) { this.speaker.value = previous; this.report(error); }
    };
    this.check.onclick = () => this.checkMicrophone().catch(error => this.report(error));
    root.querySelector('#test-speaker').onclick = () => this.testSpeaker().catch(error => this.report(error));
    navigator.mediaDevices?.addEventListener('devicechange', () => this.refresh().catch(error => this.report(error)));
    root.addEventListener('toggle', () => { if (root.open) this.refresh().catch(error => this.report(error)); });
    this.refresh().catch(error => this.report(error));
  }
  save() { try { localStorage.setItem('parley.audio', JSON.stringify(this.preferences)); } catch {} }
  report(error) { this.status.textContent = error.name === 'NotAllowedError'
    ? 'Allow microphone access in Chrome’s site settings, then check again.'
    : error.name === 'NotFoundError' || error.name === 'OverconstrainedError'
      ? 'That audio device is unavailable. Choose another device.' : error.message; }
  async refresh() {
    if (!navigator.mediaDevices?.enumerateDevices) { this.status.textContent = 'Audio devices require a secure browser connection.'; return; }
    const devices = await navigator.mediaDevices.enumerateDevices();
    for (const [select, kind, key, title] of [
      [this.mic, 'audioinput', 'microphoneId', 'Microphone'], [this.speaker, 'audiooutput', 'speakerId', 'Speaker'],
    ]) {
      const relevant = devices.filter(device => device.kind === kind && device.deviceId && device.deviceId !== 'communications');
      const defaultDevice = relevant.find(device => device.deviceId === 'default');
      select.replaceChildren(new Option(defaultDevice?.label || 'System default', ''));
      for (const [index, device] of relevant.filter(device => device.deviceId !== 'default').entries()) {
        select.add(new Option(device.label || `${title} ${index + 1} · check microphone to reveal names`, device.deviceId));
      }
      const selected = this.preferences[key];
      if (selected && ![...select.options].some(option => option.value === selected)) select.add(new Option('Previously selected device · check availability', selected));
      select.value = selected;
    }
    this.speaker.disabled = typeof this.remoteAudio.setSinkId !== 'function';
    if (this.speaker.disabled) this.status.textContent = 'This browser uses the speaker selected in your computer’s sound settings.';
  }
  useCallStream(stream) {
    this.stopCheck(); this.callStream = stream;
    this.check.disabled = !!stream;
    if (stream) this.monitor(stream);
    else { this.stopMeter(); this.label.textContent = 'Microphone inactive. Check it before answering.'; }
    this.refresh().catch(error => this.report(error));
  }
  async checkMicrophone() {
    if (this.checking || this.callStream) return;
    if (this.testStream) { this.stopCheck(); return; }
    this.checking = true; this.check.disabled = true;
    const version = this.checkVersion = (this.checkVersion || 0) + 1;
    try {
      const stream = await openMicrophone(this.preferences.microphoneId);
      if (this.checkVersion !== version || this.callStream) { stream.getTracks().forEach(track => track.stop()); return; }
      this.testStream = stream;
      await this.refresh(); this.monitor(this.testStream);
      this.check.textContent = 'Stop mic check'; this.status.textContent = 'Speak and watch the level. Your microphone is not played through speakers.';
      this.checkTimer = setTimeout(() => this.stopCheck(), 15000);
    } catch (error) { this.stopCheck(); throw error; }
    finally { this.checking = false; this.check.disabled = !!this.callStream; }
  }
  monitor(stream) {
    this.stopMeter();
    const track = stream.getAudioTracks()[0];
    this.label.textContent = `Using ${track?.label || 'selected microphone'}`;
    try {
      const context = new AudioContext(); this.context = context;
      context.resume().catch(() => {});
      const source = context.createMediaStreamSource(stream), analyser = context.createAnalyser();
      analyser.fftSize = 1024; source.connect(analyser);
      const samples = new Float32Array(analyser.fftSize);
      this.meterTimer = setInterval(() => {
        analyser.getFloatTimeDomainData(samples);
        const rms = Math.sqrt(samples.reduce((sum, value) => sum + value * value, 0) / samples.length);
        this.meter.value = Math.min(100, rms * 500);
        this.label.textContent = track?.muted || track?.enabled === false || track?.readyState === 'ended'
          ? 'Microphone is not supplying audio. Check the device or choose another microphone.'
          : `Using ${track?.label || 'selected microphone'}${rms > 0.01 ? ' · receiving sound' : ' · quiet'}`;
      }, 100);
    } catch (error) { this.report(error); }
  }
  stopMeter() {
    clearInterval(this.meterTimer); this.meterTimer = null;
    this.context?.close().catch(() => {}); this.context = null; this.meter.value = 0;
  }
  stopCheck() {
    this.checkVersion = (this.checkVersion || 0) + 1;
    clearTimeout(this.checkTimer); this.checkTimer = null;
    if (this.testStream) {
      this.testStream.getTracks().forEach(track => track.stop()); this.testStream = null;
      this.stopMeter(); this.label.textContent = 'Microphone check stopped.';
    }
    this.check.textContent = 'Check microphone';
  }
  async testSpeaker() {
    this.testAudio?.pause(); if (this.testUrl) URL.revokeObjectURL(this.testUrl);
    const rate = 16000, length = rate * 0.6, bytes = new ArrayBuffer(44 + length * 2), view = new DataView(bytes);
    const ascii = (at, text) => [...text].forEach((char, i) => view.setUint8(at + i, char.charCodeAt(0)));
    ascii(0, 'RIFF'); view.setUint32(4, bytes.byteLength - 8, true); ascii(8, 'WAVE'); ascii(12, 'fmt ');
    view.setUint32(16, 16, true); view.setUint16(20, 1, true); view.setUint16(22, 1, true);
    view.setUint32(24, rate, true); view.setUint32(28, rate * 2, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true);
    ascii(36, 'data'); view.setUint32(40, length * 2, true);
    for (let i = 0; i < length; i++) view.setInt16(44 + i * 2, 3000 * Math.sin(2 * Math.PI * 440 * i / rate) * Math.min(1, i / 200, (length - i) / 200), true);
    const element = new Audio(); this.testAudio = element;
    this.testUrl = URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' })); element.src = this.testUrl;
    if (typeof element.setSinkId === 'function') await element.setSinkId(this.preferences.speakerId);
    await element.play(); this.status.textContent = 'Playing a short tone on the selected speaker.';
    element.onended = () => { URL.revokeObjectURL(this.testUrl); this.testUrl = null; };
  }
  close() {
    this.stopCheck(); this.stopMeter(); this.testAudio?.pause();
    if (this.testUrl) URL.revokeObjectURL(this.testUrl); this.testUrl = null;
  }
}
