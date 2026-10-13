import { test, expect } from '@playwright/test';

test('incoming calls surface answer controls; active calls expose mute, transfer and hangup', async ({ page }) => {
  await page.route('**/v1/conference/browser/**', route => {
    const path = new URL(route.request().url()).pathname;
    const data = path.endsWith('/phones') ? { phones: { owner: '+12025550190', booker: '+12025550192', organizer: '+12025550191' } }
      : path.endsWith('/access') ? { authenticated: true }
      : { authenticated: true, current: { cid: 'conv_ui', url: 'ws://fixture', token: 'fixture', worker_ready: true }, conversations: [] };
    return route.fulfill({ json: data });
  });
  await page.route('**/uctp-client/client.mjs', route => route.fulfill({ contentType: 'text/javascript', body: `
    const members = ['owner','companion','booker','organizer','assistant'].map(role => ({ participant_id:role, subject:role, role, name:role === 'organizer' ? 'Jeff' : role === 'assistant' ? 'David' : role, sip:'sip:+12025550190@example.invalid' }));
    const events = [
      ['session.invited', {sid:'s1',connid:'phone',participant_id:'organizer',initiated_by:'assistant'}],
      ['connection.connected', {sid:'s1',connid:'phone',state:'connected'}],
      ['session.assistant_attached', {sid:'s1',connid:'ai'}],
      ['message.accepted', {from:'assistant',to:['owner'],delivery:'chat',content_type:'application/json',msg_id:'invite',body:JSON.stringify({type:'travel.browser_invitation',version:1,sid:'s1',retained_connid:'phone'})}]
    ];
    window.callFixture = { phase:'waiting', muted:false, moves:0 };
    export class UctpClient {
      constructor(){this.identity='owner';this.authenticated=true;}
      async connect(){this.authenticated=true;} close(){}
      command(type,cid,payload,extra){return {type,cid,payload,...extra};}
      async request(frame){ if(frame.type==='session.end') window.callFixture.phase='ended'; return {payload:{}}; }
      async snapshot(){
        const all = window.callFixture.phase === 'waiting' ? events.slice(0,3) : [...events];
        if (['consulting','active','ended'].includes(window.callFixture.phase)) all.push(['browser.consulting',{sid:'s1',connid:'web',state:'consulting',details:{held_connid:'phone',bridge_id:'consult'}}]);
        if(['active','ended'].includes(window.callFixture.phase)) all.push(['browser.speaking',{sid:'s1',connid:'web',state:'speaking',details:{retained_connid:'phone',bridge_id:'bridge'}}]);
        if(window.callFixture.phase === 'ended') all.push(['session.ended',{sid:'s1',state:'ended'}]);
        return {state:'open',participants:members,capabilities:{sms_mode:'fake',browser_handoff:true,phone_handoff:true},events:all.map(([event_type,payload],i)=>({seq:i+1,cid:'conv_ui',event_type,payload,created_at:new Date().toISOString()}))};
      }
    }
  ` }));
  await page.route('**/uctp-client/browser-audio.mjs', route => route.fulfill({ contentType: 'text/javascript', body: `
    export async function openMicrophone(){ throw new Error("No microphone needed for UI fixture"); }
    export class BrowserAudio {
      constructor(){ this.media=null; this.muted=false; }
      async join(){this.media={};window.callFixture.phase='consulting';}
      setMuted(value){this.muted=value;window.callFixture.muted=value;}
      async moveToPhone(){window.callFixture.moves++;}
      closeLocal(){this.media=null;} resetPhoneMove(){} async setMicrophone(){} async setSpeaker(){}
    }
  ` }));
  await page.goto('/conference/');
  await expect(page.locator('#settings-panel')).toBeHidden();
  await expect(page.locator('#status')).toContainText('Connected');
  await expect(page.locator('#call-panel')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Hang up', exact: true })).toBeHidden();
  await page.evaluate(() => { (window as any).callFixture.phase = 'incoming'; });
  await expect(page.locator('#call-panel')).toBeVisible();
  await expect(page.locator('#call-title')).toHaveText('David is calling');
  await expect(page.locator('#call-panel')).toHaveClass(/incoming/);
  await expect(page.locator('#move-phone')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Answer David' })).toBeEnabled();
  await page.screenshot({ path: 'test-results/conference-incoming-call.png', fullPage: true });
  await page.getByRole('button', { name: 'Not now', exact: true }).click();
  await expect(page.locator('#call-panel')).not.toHaveClass(/incoming/);
  await expect(page.locator('#join')).toBeEnabled();
  await page.locator('#join').click();
  await expect(page.locator('#call-state')).toHaveText('WITH DAVID · JEFF ON HOLD');
  await expect(page.locator('#move-phone')).toBeHidden();
  await expect(page.locator('[data-edge=rtp]')).toHaveAttribute('data-state','held');
  await page.evaluate(() => { (window as any).callFixture.phase = 'active'; });
  await expect(page.locator('#call-state')).toHaveText('CONNECTED · BROWSER AUDIO');
  await page.getByRole('button', { name: 'Mute microphone', exact: true }).click();
  expect(await page.evaluate(() => (window as any).callFixture.muted)).toBe(true);
  await expect(page.getByRole('button', { name: 'Unmute microphone', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: 'Move to my phone', exact: true }).click();
  expect(await page.evaluate(() => (window as any).callFixture.moves)).toBe(1);
  await page.screenshot({ path: 'test-results/conference-active-call.png', fullPage: true });
  await page.getByRole('button', { name: 'Hang up', exact: true }).click();
  await expect(page.locator('#call-panel')).toBeHidden();
});

test('Start calls David in the browser and hides call controls again after his callback promise', async ({page})=>{
  await page.route('**/v1/conference/browser/**',route=>route.fulfill({json:{authenticated:true,current:{cid:'intro_ui',url:'ws://fixture',token:'fixture',worker_ready:true},conversations:[]}}));
  await page.route('**/uctp-client/client.mjs',route=>route.fulfill({contentType:'text/javascript',body:`
    window.introFixture={phase:'ready',starts:0,messages:0};
    export class UctpClient {
      constructor(){this.identity='owner';this.authenticated=true;}async connect(){}close(){}
      command(type,cid,payload,extra){return {type,cid,payload,...extra};}
      async request(frame){if(frame.type==='message.send')window.introFixture.messages++;return {payload:{}};}
      async snapshot(){const f=window.introFixture;const events=f.phase==='ready'?[]:[
        ['session.invited',{sid:'intro',connid:'web',participant_id:'owner',initiated_by:'owner',transport:'webrtc'}],
        ['connection.offered',{sid:'intro',connid:'web',transport:'webrtc'}],
        ['browser.answered',{sid:'intro',connid:'web',state:'answered'}],
        ['session.assistant_attached',{sid:'intro',connid:'ai'}],
        ...(f.phase==='ended'?[['session.ended',{sid:'intro',state:'ended'}]]:[])];
        return {state:'open',capabilities:{sms_mode:'fake',browser_handoff:true},participants:[{participant_id:'owner',subject:'owner',role:'owner',name:'Jonathan'},{participant_id:'ai',role:'assistant',name:'David'}],events:events.map(([event_type,payload],i)=>({seq:i+1,cid:'intro_ui',event_type,payload,created_at:new Date().toISOString()}))};}
    }
  `}));
  await page.route('**/uctp-client/browser-audio.mjs',route=>route.fulfill({contentType:'text/javascript',body:`
    export async function openMicrophone(){}
    export class BrowserAudio {async callDavid(purpose){if(!purpose)throw Error('Missing travel context');this.media={};this.peer={};window.introFixture.starts++;window.introFixture.phase='calling';}closeLocal(){this.media=null;this.peer=null;}async setSpeaker(){}async setMicrophone(){}resetPhoneMove(){}}
  `}));
  await page.goto('/conference/');await expect(page.locator('#status')).toContainText('Connected');
  await expect(page.locator('#call-panel')).toBeHidden();
  await page.getByRole('button',{name:'Call David · Start demo'}).click();
  await expect(page.locator('#call-state')).toHaveText('WITH DAVID');
  await expect(page.locator('#call-title')).toHaveText('David');
  await expect(page.locator('#join')).toBeHidden();await expect(page.locator('#move-phone')).toBeHidden();
  await expect(page.locator('#mute-call')).toBeVisible();await expect(page.locator('#end')).toBeVisible();
  expect(await page.evaluate(()=>(window as any).introFixture.starts)).toBe(1);
  expect(await page.evaluate(()=>(window as any).introFixture.messages)).toBe(0);
  await page.evaluate(()=>{(window as any).introFixture.phase='ended';});
  await expect(page.locator('#call-panel')).toBeHidden();
});
