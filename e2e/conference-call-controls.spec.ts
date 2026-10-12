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
    const members = ['owner','companion','booker','organizer','assistant'].map(role => ({ participant_id:role, subject:role, role, name:role === 'organizer' ? 'Conference organizer' : role, sip:'sip:+12025550190@example.invalid' }));
    const events = [
      ['session.invited', {sid:'s1',connid:'phone',participant_id:'organizer',initiated_by:'assistant'}],
      ['connection.connected', {sid:'s1',connid:'phone',state:'connected'}],
      ['session.assistant_attached', {sid:'s1',connid:'ai'}],
      ['message.accepted', {from:'assistant',to:['owner'],delivery:'chat',content_type:'application/json',msg_id:'invite',body:JSON.stringify({type:'travel.browser_invitation',version:1,sid:'s1',retained_connid:'phone'})}]
    ];
    window.callFixture = { phase:'incoming', muted:false, moves:0 };
    export class UctpClient {
      constructor(){this.identity='owner';this.authenticated=true;}
      async connect(){this.authenticated=true;} close(){}
      command(type,cid,payload,extra){return {type,cid,payload,...extra};}
      async request(frame){ if(frame.type==='session.end') window.callFixture.phase='ended'; return {payload:{}}; }
      async snapshot(){
        const all = [...events];
        if(window.callFixture.phase !== 'incoming') all.push(['browser.speaking',{sid:'s1',connid:'web',state:'speaking',details:{retained_connid:'phone',bridge_id:'bridge'}}]);
        if(window.callFixture.phase === 'ended') all.push(['session.ended',{sid:'s1',state:'ended'}]);
        return {state:'open',participants:members,capabilities:{sms_mode:'fake',browser_handoff:true,phone_handoff:true},events:all.map(([event_type,payload],i)=>({seq:i+1,cid:'conv_ui',event_type,payload,created_at:new Date().toISOString()}))};
      }
    }
  ` }));
  await page.route('**/uctp-client/browser-audio.mjs', route => route.fulfill({ contentType: 'text/javascript', body: `
    export async function openMicrophone(){ throw new Error("No microphone needed for UI fixture"); }
    export class BrowserAudio {
      constructor(){ this.media=null; this.muted=false; }
      async join(){this.media={};window.callFixture.phase='active';}
      setMuted(value){this.muted=value;window.callFixture.muted=value;}
      async moveToPhone(){window.callFixture.moves++;}
      closeLocal(){this.media=null;} resetPhoneMove(){} async setMicrophone(){} async setSpeaker(){}
    }
  ` }));
  await page.goto('/conference/');
  await expect(page.locator('#settings-panel')).toBeHidden();
  await expect(page.locator('#call-panel')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Answer organizer call' })).toBeEnabled();
  await page.screenshot({ path: 'test-results/conference-incoming-call.png', fullPage: true });
  await page.getByRole('button', { name: 'Not now', exact: true }).click();
  await expect(page.locator('#call-panel')).not.toHaveClass(/incoming/);
  await expect(page.locator('#join')).toBeEnabled();
  await page.locator('#join').click();
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
