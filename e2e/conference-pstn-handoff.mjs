// Real stage/media test. A phone holder must separately confirm what they heard.
// A synthetic SIP fixture can exercise this harness, but is explicitly labeled.
import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {UctpClient} from '../clients/uctp-js/client.mjs';
import {chromium, expect} from '@playwright/test';
const fixture = JSON.parse(process.env.LIVE_VOICE_FIXTURE);
const pcm = await readFile(fixture.speechPcm);
if (pcm.length < 16000 || pcm.length > 720000 || pcm.length % 2) throw new Error('Expected bounded 8 kHz mono PCM speech');
function wavHeader(bytes, rate) {
  const header=Buffer.alloc(44);header.write('RIFF');header.writeUInt32LE(bytes+36,4);header.write('WAVEfmt ',8);
  header.writeUInt32LE(16,16);header.writeUInt16LE(1,20);header.writeUInt16LE(1,22);header.writeUInt32LE(rate,24);
  header.writeUInt32LE(rate*2,28);header.writeUInt16LE(2,32);header.writeUInt16LE(16,34);header.write('data',36);header.writeUInt32LE(bytes,40);return header;
}
const speech=Buffer.concat([wavHeader(pcm.length,8000),pcm]).toString('base64');
const browser=await chromium.launch({channel:process.env.PARLEY_BROWSER_CHANNEL==='chromium'?'chromium':'chrome',
  args:['--autoplay-policy=no-user-gesture-required','--use-fake-ui-for-media-stream','--use-fake-device-for-media-stream']});
let page;
try {
  page=await browser.newPage({viewport:{width:1600,height:1100}});
  await page.goto(`${fixture.http}/conference/`);
  await page.evaluate(synthetic => {
    const label=document.createElement('p');label.textContent=synthetic
      ? 'HARNESS CHECK · Synthetic SIP endpoint, generated browser speech. No PSTN or SMS.'
      : 'PSTN HANDOFF TEST · Real telephone call; generated browser speech. Listening confirmation required.';
    label.style.cssText='margin:4px 0 0;color:#f6c986;font-size:10px';document.querySelector('header > div').append(label);
    document.getElementById('remote-audio').muted=true;
  },Boolean(fixture.syntheticSip));
  await page.locator('#url').fill(fixture.url);await page.locator('#cid').fill(fixture.cid);await page.locator('#token').fill(fixture.token);
  const controlStart=Date.now();
  await page.getByRole('button',{name:'Connect',exact:true}).click();
  // The control client's own connection deadline is 20 seconds. Do not
  // declare a voice failure while its authenticated setup is still pending.
  await expect(page.locator('#setup')).toBeHidden({timeout:30000});
  console.log(JSON.stringify({event:'pstn.stage_control.connected',elapsed_ms:Date.now()-controlStart}));
  await expect(page.locator('#join')).toBeEnabled({timeout:15000});
  await page.evaluate(async base64 => {
    const NativePeer=window.RTCPeerConnection;
    window.RTCPeerConnection=class extends NativePeer {constructor(...args){super(...args);window.probePeer=this;}};
    const source=new AudioContext();await source.resume();
    const bytes=Uint8Array.from(atob(base64),c=>c.charCodeAt(0));
    const buffer=await source.decodeAudioData(bytes.buffer);
    const destination=source.createMediaStreamDestination();
    // A microphone remains live between utterances. Keep this generated
    // microphone live too; an ended BufferSource otherwise stops producing
    // frames and accidentally simulates a lost microphone after the phrase.
    const silence=source.createConstantSource();silence.offset.value=0;
    silence.connect(destination);silence.start();
    window.probeMicrophone={source,silence,destination};
    navigator.mediaDevices.getUserMedia=async()=>destination.stream;
    window.startProbeSpeech=()=>{const node=source.createBufferSource();node.buffer=buffer;node.connect(destination);node.start();return buffer.duration;};
  },speech);
  const start=Date.now();await page.locator('#join').click();
  await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state','speaking',{timeout:20000});
  await expect(page.locator('[data-edge=vapi]')).toHaveAttribute('data-state','retired');
  await expect(page.locator('#retained')).toContainText('Same telephone Connection retained');
  await expect(page.locator('#network-ids')).toContainText(fixture.connid);await expect(page.locator('#network-ids')).toContainText(fixture.sid);
  const handoffMs=Date.now()-start;
  // Save actual decoded telephone audio. Silence packets are not human replies.
  const recording=await page.evaluate(async () => {
    const peer=window.probePeer;
    const statsBefore=await peer.getStats();
    const transport=[...statsBefore.values()].find(s=>s.type==='transport'&&s.selectedCandidatePairId);
    const pair=statsBefore.get(transport?.selectedCandidatePairId),remote=statsBefore.get(pair?.remoteCandidateId);
    const sink=new AudioContext();await sink.resume();
    const stream=document.getElementById('remote-audio').srcObject;if(!stream)throw new Error('No remote audio stream');
    const input=sink.createMediaStreamSource(stream),capture=sink.createScriptProcessor(4096,1,1),mute=sink.createGain();mute.gain.value=0;
    input.connect(capture);capture.connect(mute).connect(sink.destination);
    const chunks=[];let voiced=0,responseBlocks=0,peak=0;
    const duration=window.startProbeSpeech(),began=performance.now();
    capture.onaudioprocess=event=>{
      const samples=new Float32Array(event.inputBuffer.getChannelData(0));chunks.push(samples);
      const rms=Math.sqrt(samples.reduce((sum,v)=>sum+v*v,0)/samples.length);peak=Math.max(peak,rms);
      if(rms>0.01){voiced++;if(performance.now()-began>Math.min(duration,4)*1000)responseBlocks++;}
    };
    await new Promise(resolve=>setTimeout(resolve,Math.min(55000,(duration+12)*1000)));
    capture.onaudioprocess=null;input.disconnect();capture.disconnect();mute.disconnect();
    const stats=await peer.getStats();let inbound=0,outbound=0,codec;
    for(const stat of stats.values()){
      if(stat.type==='inbound-rtp'&&stat.kind==='audio')inbound=stat.packetsReceived;
      if(stat.type==='outbound-rtp'&&stat.kind==='audio'){outbound=stat.packetsSent;codec=stats.get(stat.codecId)?.mimeType;}
    }
    const count=chunks.reduce((sum,chunk)=>sum+chunk.length,0),bytes=new Uint8Array(count*2),view=new DataView(bytes.buffer);let offset=0;
    for(const chunk of chunks)for(const sample of chunk){view.setInt16(offset,Math.round(Math.max(-1,Math.min(1,sample))*32767),true);offset+=2;}
    let binary='';for(let pos=0;pos<bytes.length;pos+=16384)binary+=String.fromCharCode(...bytes.subarray(pos,pos+16384));
    await sink.close();
    return {pcm:btoa(binary),sampleRate:sink.sampleRate,voiced,responseBlocks,peak,inbound,outbound,codec,
      speechDuration:duration,connectionAtHandoff:transport?.iceState,dtls:transport?.dtlsState,
      remote:remote&&{address:remote.address,port:remote.port,protocol:remote.protocol,candidateType:remote.candidateType}};
  });
  const {pcm:captured,...measured}=recording,raw=Buffer.from(captured,'base64');
  await writeFile(`${fixture.output}/telephone-return.wav`,Buffer.concat([wavHeader(raw.length,measured.sampleRate),raw]),{mode:0o600});
  await writeFile(`${fixture.output}/browser-result.json`,JSON.stringify({handoffMs,measured,
    synthetic_sip:Boolean(fixture.syntheticSip),human_listening_verified:false},null,2),{mode:0o600});
  await page.screenshot({path:`${fixture.output}/handoff.png`,fullPage:true});
  assert.equal(measured.dtls,'connected');assert.equal(measured.codec?.toLowerCase(),'audio/opus');
  assert.equal(measured.remote?.address,fixture.publicIp);assert.equal(measured.remote?.protocol,'udp');
  assert.ok(measured.inbound>20&&measured.outbound>20,'RTP packets required in both directions');
  assert.ok(measured.responseBlocks>=5,'Decoded telephone audio required in the response window');
  console.log(JSON.stringify({event:'pstn.browser_handoff.measured',handoff_ui_ms:handoffMs,
    inbound_packets:measured.inbound,outbound_packets:measured.outbound,
    decoded_audio_blocks_in_response_window:measured.responseBlocks,human_listening_verified:false,synthetic_sip:Boolean(fixture.syntheticSip)}));
  if(fixture.holdAfterBrowserMs){
    console.log(JSON.stringify({event:'pstn.diagnostic_hold.started',hold_ms:fixture.holdAfterBrowserMs}));
    await page.waitForTimeout(fixture.holdAfterBrowserMs);
    await expect(page.locator('[data-edge=sip]')).toHaveAttribute('data-state','connected');
  }
  if(fixture.moveToPhone){
    await expect(page.locator('#move-phone')).toBeEnabled();
    const control=new UctpClient(fixture.url,fixture.token,{timeoutMs:20000});
    try {
      await control.connect();
      const before=await control.snapshot(fixture.cid);
      const browserEvent=before.events.find(e=>e.event_type==='browser.speaking'&&e.payload.sid===fixture.sid);
      assert.ok(browserEvent,'Committed browser route required');
      console.log(JSON.stringify({event:'pstn.phone_move.starting',instruction:'Answer the callback on the 3669 phone and press 1. Keep the 0737 phone on the existing call.'}));
      const moveStart=Date.now();await page.locator('#move-phone').click();
      await expect(page.locator('[data-edge=phone]')).toHaveAttribute('data-state','speaking',{timeout:50000});
      await expect(page.locator('[data-edge=browser]')).toHaveAttribute('data-state','retired');
      const journal=await control.snapshot(fixture.cid);
      const moved=journal.events.find(e=>e.event_type==='phone.speaking'&&e.payload.sid===fixture.sid);
      assert.ok(moved);assert.equal(moved.cid,fixture.cid);
      assert.equal(moved.payload.participant_id,fixture.ownerPid);
      assert.equal(moved.payload.details.retained_connid,fixture.connid);
      assert.equal(moved.payload.details.retired_connid,browserEvent.payload.connid);
      assert.equal(moved.payload.details.join_confirmed,true);
      assert.equal(journal.events.filter(e=>e.event_type==='phone.prepared').length,1);
      await writeFile(`${fixture.output}/phone-result.json`,JSON.stringify({phone_move_ms:Date.now()-moveStart,cid:fixture.cid,sid:fixture.sid,
        owner_pid:fixture.ownerPid,callback_connid:moved.payload.connid,retained_connid:fixture.connid,human_listening_verified:false},null,2),{mode:0o600});
      await page.screenshot({path:`${fixture.output}/phone-move.png`,fullPage:true});
      const phoneHoldMs=fixture.syntheticSip?25000:65000;
      console.log(JSON.stringify({event:'pstn.phone_move.committed',hold_ms:phoneHoldMs,instruction:'Keep both calls open. Say green lantern on one phone and silver mountain on the other. Confirm hearing both directions; the live check holds past 60 seconds.'}));
      await page.waitForTimeout(phoneHoldMs);
      await expect(page.locator('[data-edge=phone]')).toHaveAttribute('data-state','speaking');
      await expect(page.locator('[data-edge=sip]')).toHaveAttribute('data-state','connected');
    }finally{control.close();}
  }
  if(await page.locator('#end').isEnabled())await page.locator('#end').click();
} catch(error) {
  if(page){await page.screenshot({path:`${fixture.output}/failure.png`,fullPage:true}).catch(()=>{});
    await writeFile(`${fixture.output}/failure.json`,JSON.stringify({error:error.message,notice:await page.locator('#notice').textContent().catch(()=>null)},null,2),{mode:0o600});}
  throw error;
} finally {await browser.close();}
