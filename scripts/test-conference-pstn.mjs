// Explicit opt-in real call. Provisioning uses admin REST; communications use UCTP.
// Usage: node scripts/test-conference-pstn.mjs --run private-contact.json
import {readFile,mkdir,writeFile} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {UctpClient,redact} from '../clients/uctp-js/client.mjs';
import {verifyProviderRetirement,verifyCallsEnded,ssm} from '../e2e/support/cloud-probe.mjs';
const [action,configPath]=process.argv.slice(2);
if(action!=='--run'||!configPath)throw new Error('Usage: node scripts/test-conference-pstn.mjs --run private-contact.json (places real calls to the private configured phones)');
const config=JSON.parse(await readFile(configPath,'utf8'));
if(!/^\+[1-9][0-9]{7,14}$/.test(config.phone||'')||!config.speechPcm)throw new Error('Private config requires E.164 phone and speechPcm path');
if(config.callback_phone&&(!/^\+[1-9][0-9]{7,14}$/.test(config.callback_phone)||config.callback_phone===config.phone))throw new Error('callback_phone must be a distinct consenting E.164 phone');
if(config.hold_after_browser_ms!=null&&(!Number.isInteger(config.hold_after_browser_ms)||config.hold_after_browser_ms<0||config.hold_after_browser_ms>90000))throw new Error('Invalid diagnostic hold duration');
const speech=await readFile(config.speechPcm);
if(speech.length<16000||speech.length>720000||speech.length%2)throw new Error('Expected bounded raw 8 kHz mono speech');
for(const key of ['PARLEY_API_SECRET','VAPI_PRIVATE_KEY','VAPI_ASSISTANT_ID'])if(!process.env[key])throw new Error(`${key} required`);
const http='https://conference.rudeless.ai',url='wss://conference.rudeless.ai/uctp';
const output=`var/conference/live/pstn-${Date.now().toString(36)}`;
await mkdir(output,{recursive:true,mode:0o700});
const save=(name,value)=>writeFile(`${output}/${name}.json`,JSON.stringify(value,null,2),{mode:0o600});
const admin=new UctpClient(url,process.env.PARLEY_API_SECRET,{timeoutMs:20000});
let owner,assistant,fixture,failure,browserPassed=false;
async function connectControl(client){
  // Establishing/authenticating control has no provider effects. Retry only
  // this step; never retry a call invitation with a fresh command identity.
  for(let attempt=0;;attempt++){
    try{return await client.connect();}
    catch(error){client.close();if(attempt===2)throw error;await new Promise(resolve=>setTimeout(resolve,250));}
  }
}
async function request(client,name,command){await save(`${name}-command`,command);const response=await client.request(command);await save(`${name}-response`,redact(response));return response;}
try {
  await connectControl(admin);
  const created=await request(admin,'create',admin.command('conversation.create',null,{participants:[
    {alias:'jonathan',name:'Jonathan — PSTN test',role:'owner',...(config.callback_phone?{sip:`sip:${config.callback_phone}@sip.telnyx.com`}:{})},
    {alias:'organizer',name:'Jonathan — test telephone',role:'organizer',sip:`sip:${config.phone}@sip.telnyx.com`},
    {alias:'assistant',name:'Vapi assistant',role:'assistant'}]}));
  const participants=created.payload.participants;
  for(const member of participants.filter(p=>['owner','assistant'].includes(p.role))){
    const res=await fetch(`${http}/v1/conference/${created.cid}/tokens`,{method:'POST',redirect:'error',
      headers:{authorization:`Bearer ${process.env.PARLEY_API_SECRET}`,'content-type':'application/json'},
      body:JSON.stringify({participant_id:member.participant_id}),signal:AbortSignal.timeout(10000)});
    if(!res.ok)throw new Error(`Token provisioning ${res.status}`);member.token=(await res.json()).token;
  }
  const ownerMember=participants.find(p=>p.role==='owner');
  owner=new UctpClient(url,ownerMember.token,{timeoutMs:20000});
  assistant=new UctpClient(url,participants.find(p=>p.role==='assistant').token,{timeoutMs:20000});
  await connectControl(owner);await connectControl(assistant);
  fixture={cid:created.cid,http,url,token:ownerMember.token,output,publicIp:'32.185.99.169',speechPcm:config.speechPcm,syntheticSip:false,moveToPhone:Boolean(config.callback_phone),ownerPid:ownerMember.participant_id,holdAfterBrowserMs:config.hold_after_browser_ms||0};
  await save('fixture',fixture);
  const invited=await request(assistant,'invite',assistant.command('session.invite',fixture.cid,{medium:'voice',
    to:participants.find(p=>p.role==='organizer').participant_id,
    purpose:'This is an authorized Rudeless connectivity test with Jonathan on his telephone. Tell him this is the connectivity test and ask him to say blue umbrella. Once he says it, thank him and ask him to stay on the line. His browser will take over this SAME call and say a different confirmation code. He should repeat that code back and say whether he heard it clearly. Do not end this call yourself, call anyone else, send messages, or discuss actual travel bookings.'}));
  fixture.sid=invited.payload.session.sid;fixture.connid=invited.payload.session.connid;await save('fixture',fixture);
  console.log(JSON.stringify({event:'pstn.call.started',cid:fixture.cid,sid:fixture.sid,output,
    instruction:'Answer the telephone and say blue umbrella when the assistant asks. The browser then speaks a second code.'}));
  const deadline=Date.now()+75000;
  for(;;){
    const journal=await owner.snapshot(fixture.cid);await save('before-browser',redact({type:'conference.snapshot',...journal}));
    const human=journal.events.filter(e=>e.event_type==='session.transcript'&&e.payload.sid===fixture.sid&&e.payload.speaker!==participants.find(p=>p.role==='assistant').participant_id).map(e=>e.payload.text).join(' ');
    if(journal.events.some(e=>['session.ended','session.failed'].includes(e.event_type)&&e.payload.sid===fixture.sid))throw new Error('Telephone call ended before the human readiness phrase');
    if(/blue\s+umbrella/i.test(human)&&journal.events.some(e=>e.event_type==='session.assistant_attached'&&e.payload.sid===fixture.sid))break;
    if(/(?:record\s+your\s+message|leave\s+(?:a|your)\s+message|voicemail)/i.test(human))throw new Error('Voicemail detected; browser takeover was not attempted');
    if(Date.now()>deadline)throw new Error('No verified human readiness phrase; browser takeover was not attempted');
    await new Promise(resolve=>setTimeout(resolve,500));
  }
  await new Promise((resolve,reject)=>{const child=spawn(process.execPath,['e2e/conference-pstn-handoff.mjs'],{stdio:'inherit',env:{...process.env,
    LIVE_VOICE_FIXTURE:JSON.stringify(fixture),PLAYWRIGHT_BROWSERS_PATH:'var/conference-browsers',PARLEY_BROWSER_CHANNEL:'chromium'}});
    child.on('error',reject);child.on('exit',code=>code===0?resolve():reject(new Error(`PSTN browser gate exited ${code}`)));});
  browserPassed=true;
} catch(error){failure=error;console.error(error.message);}
finally {
  if(owner&&fixture){
    try {
      if(!owner.authenticated)await connectControl(owner);
      if(fixture.sid)await request(owner,'cleanup-end',owner.command('session.end',fixture.cid,{}, {sid:fixture.sid}));
      const journal=await owner.snapshot(fixture.cid);await save('journal',redact({type:'conference.snapshot',...journal}));
      if(browserPassed){
        await save('vapi-retirement',await verifyProviderRetirement(fixture,journal));
        if(fixture.moveToPhone){
          const moved=journal.events.find(e=>e.event_type==='phone.speaking'&&e.payload.sid===fixture.sid);
          if(!moved||!journal.events.some(e=>e.event_type==='session.ended'&&e.payload.sid===fixture.sid))throw new Error('Both-phone Session teardown was not confirmed');
          if(!/^sess_[a-f0-9]{32}$/.test(fixture.sid)||!/^conn_[a-f0-9]{32}$/.test(moved.payload.connid))throw new Error('Invalid server-issued identifiers');
          const teardown=JSON.parse(await ssm([`python3 - <<'PY'
import sqlite3,json
with sqlite3.connect('file:/opt/parley/state/parley.sqlite?mode=ro',uri=True) as db:
 sid='${fixture.sid}'
 print(json.dumps({'active_connections':db.execute("SELECT count(*) FROM connections WHERE session_id=? AND state NOT IN ('ended','failed')",(sid,)).fetchone()[0], 'callback_state':db.execute("SELECT state FROM conference_phone_moves WHERE connection_id=?",('${moved.payload.connid}',)).fetchone()[0]}))
PY`]));
          await save('phone-teardown',teardown);
          if(teardown.active_connections!==0||teardown.callback_state!=='ended')throw new Error('Telephone resources remain unsettled');
        }
      }
      else if(journal.events.some(e=>e.event_type==='session.assistant_attached'&&e.payload.sid===fixture.sid))
        await save('vapi-status',await verifyCallsEnded(fixture));
    }catch(error){failure??=error;console.error(`Cleanup verification: ${error.message}`);}
  }
  owner?.close();assistant?.close();admin.close();
}
await save('result',{automated_handoff_verified:browserPassed&&!failure,automated_phone_move_verified:Boolean(config.callback_phone)&&browserPassed&&!failure,human_listening_verified:false,
  status:failure?'failed':'listening-confirmation-required',error:failure?.message,
  confirmation_required:['Telephone holder heard orange bicycle from the browser','Recorded browser return audio contains the telephone holder repeating orange bicycle',...(config.callback_phone?['After pressing 1, each phone holder heard the other speak clearly in both directions']:[])],
  completed_at:new Date().toISOString()});
if(failure)process.exitCode=1;
else console.log(JSON.stringify({event:'pstn.listening_confirmation_required',output,recording:`${output}/telephone-return.wav`}));
