// Unlock sound during a user gesture; ring only for an observed live invitation.
export class BrowserRingtone {
  constructor({ AudioContextImpl = globalThis.AudioContext,
    schedule = (callback, delay) => globalThis.setInterval(callback, delay),
    cancel = id => globalThis.clearInterval(id) } = {}) {
    this.AudioContext = AudioContextImpl; this.schedule = schedule; this.cancel = cancel;
    this.context = null; this.sid = null; this.timer = null; this.tones = new Set(); this.answered = new Set();
  }
  unlock() {
    try {
      this.context ||= new this.AudioContext();
      // A browser may leave resume pending while audio is unavailable. Request
      // sound during the gesture without holding up Connect or the owner task.
      Promise.resolve(this.context.resume()).catch(() => {});
    } catch { /* The visible invitation remains available. */ }
  }
  start(sid) {
    if (this.sid === sid) return;
    this.stop();
    if (!sid || this.answered.has(sid)) return;
    this.sid = sid;
    this.beep(); this.timer = this.schedule(() => this.beep(), 4000);
  }
  beep() {
    const ctx = this.context;
    if (!this.sid || ctx?.state !== 'running') return;
    const gain = ctx.createGain(); gain.gain.value = 0.06; gain.connect(ctx.destination);
    for (const frequency of [440, 480]) {
      const tone = ctx.createOscillator(); tone.frequency.value = frequency; tone.connect(gain);
      tone.onended = () => { tone.disconnect(); this.tones.delete(tone); };
      this.tones.add(tone); tone.start(); tone.stop(ctx.currentTime + 0.8);
    }
    // Disconnect each completed burst without leaving nodes on the output graph.
    const last = [...this.tones].at(-1), ended = last.onended;
    last.onended = () => { ended(); gain.disconnect(); };
  }
  answer(sid) { this.answered.add(sid); this.stop(); }
  stop() {
    if (this.timer !== null) this.cancel(this.timer);
    this.timer = null; this.sid = null;
    for (const tone of this.tones) { try { tone.stop(); } catch {} tone.disconnect(); }
    this.tones.clear();
  }
}
