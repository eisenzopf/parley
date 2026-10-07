/** Authorized server-offer WebRTC negotiation, entirely controlled through UCTP. */
export class BrowserAudio {
  constructor(client, cid, audioElement) {
    this.client = client; this.cid = cid; this.audio = audioElement;
  }
  async join(sid) {
    if (this.peer) throw new Error('Browser audio already active');
    this.sid = sid;
    this.media = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
    try {
      const offer = await this.client.request(this.client.command('session.update', this.cid, { kind: 'join_browser' }, { sid }));
      if (offer.type !== 'connection.offer' || offer.sid !== sid) throw new Error('Expected offer for the existing voice Session');
      this.connid = offer.connid;
      const peer = new RTCPeerConnection({ iceServers: offer.payload.ice_servers || [] }); this.peer = peer;
      peer.ontrack = event => { this.audio.srcObject = event.streams[0] || new MediaStream([event.track]); this.audio.play().catch(() => {}); };
      await peer.setRemoteDescription({ type: 'offer', sdp: offer.payload.substrate_setup.sdp });
      for (const track of this.media.getTracks()) peer.addTrack(track, this.media);
      await peer.setLocalDescription(await peer.createAnswer());
      await waitState(peer, 'icegatheringstatechange', () => peer.iceGatheringState === 'complete', 10000, 'ICE gathering');
      await this.client.request(this.client.command('connection.answer', this.cid, {
        substrate: 'webrtc', substrate_setup: { sdp_type: 'answer', sdp: peer.localDescription.sdp },
      }, { sid, connid: this.connid }));
      await waitState(peer, 'connectionstatechange', () => peer.connectionState === 'connected', 15000, 'WebRTC connection');
      // The original assistant/telephone bridge remains until this commit.
      return await this.client.request(this.client.command('session.update', this.cid, { kind: 'handoff_to_browser' }, { sid, connid: this.connid }));
    } catch (error) {
      if (this.connid && this.client.authenticated) {
        await this.client.request(this.client.command('connection.end', this.cid, {}, { sid, connid: this.connid })).catch(() => {});
      }
      this.closeLocal(); throw error;
    }
  }
  async endSession() {
    try { return await this.client.request(this.client.command('session.end', this.cid, {}, { sid: this.sid })); }
    finally { this.closeLocal(); }
  }
  async moveToPhone() {
    if (!this.peer || !this.connid) throw new Error('Join through the browser before moving to your phone');
    // Keep the exact request for an ambiguous timeout: a retry cannot redial.
    this.phoneRequest ||= this.client.command('session.update', this.cid, { kind: 'move_to_phone' }, { sid: this.sid, connid: this.connid });
    const response = await this.client.request(this.phoneRequest);
    this.phoneConnid = response.payload.session.connid;
    return response;
  }
  async cancelPhoneMove(connid = this.phoneConnid) {
    if (!connid) throw new Error('Callback connection not yet known; inspect its journal outcome before retrying');
    return this.client.request(this.client.command('session.update', this.cid, { kind: 'cancel_phone_move' }, { sid: this.sid, connid }));
  }
  resetPhoneMove() { this.phoneRequest = null; this.phoneConnid = null; }
  closeLocal() {
    this.media?.getTracks().forEach(track => track.stop()); this.peer?.close();
    this.audio.srcObject = null; this.peer = null; this.media = null; this.connid = null;
  }
}

function waitState(peer, name, ready, ms, label) {
  if (ready()) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const finish = error => { clearTimeout(timer); peer.removeEventListener(name, changed); error ? reject(error) : resolve(); };
    const changed = () => {
      if (ready()) finish();
      else if (['failed', 'closed'].includes(peer.connectionState)) finish(new Error(`${label} failed`));
    };
    const timer = setTimeout(() => finish(new Error(`${label} timed out; telephone side retained`)), ms);
    peer.addEventListener(name, changed); changed();
  });
}
