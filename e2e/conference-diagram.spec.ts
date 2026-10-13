import { test, expect } from '@playwright/test';

const journal: [string, any][] = [
  ['session.invited',{sid:'intro',connid:'opening',participant_id:'owner',initiated_by:'owner',transport:'webrtc'}],
  ['browser.answered',{sid:'intro',connid:'opening',state:'answered'}],
  ['session.assistant_attached',{sid:'intro',connid:'opening-ai',participant_id:'assistant'}],
  ['session.speech',{sid:'intro',speaker:'assistant',state:'started'}],
  ['session.ended',{sid:'intro',state:'ended'}],
  ['session.invited',{sid:'booking',connid:'reservationist',participant_id:'booker',initiated_by:'assistant'}],
  ['connection.connected',{sid:'booking',connid:'reservationist',state:'connected'}],
  ['session.assistant_attached',{sid:'booking',connid:'booking-ai'}],
  ['session.ended',{sid:'booking',state:'ended'}],
  ['message.accepted',{msg_id:'jeff-sms',from:'assistant',to:['organizer'],delivery:'sms',body:'Hey Jeff, good news. I got Jonathan and Andrew a flight. Can I call you?'}],
  ['message.received',{from:'organizer',delivery:'sms',body:'Yes, call me.'}],
  ['session.invited',{sid:'jeff-call',connid:'jeff-phone',participant_id:'organizer',initiated_by:'assistant'}],
  ['connection.connected',{sid:'jeff-call',connid:'jeff-phone',state:'connected'}],
  ['session.assistant_attached',{sid:'jeff-call',connid:'jeff-ai'}],
  ['message.accepted',{msg_id:'browser-invite',from:'assistant',to:['owner'],delivery:'chat',content_type:'application/json',body:JSON.stringify({type:'travel.browser_invitation',version:1,sid:'jeff-call',retained_connid:'jeff-phone'})}],
  ['connection.offered',{sid:'jeff-call',connid:'browser',transport:'webrtc'}],
  ['browser.answered',{sid:'jeff-call',connid:'browser',state:'answered'}],
  ['browser.consulting',{sid:'jeff-call',connid:'browser',state:'consulting',details:{held_connid:'jeff-phone',bridge_id:'private'}}],
  ['browser.speaking',{sid:'jeff-call',connid:'browser',state:'speaking',details:{retained_connid:'jeff-phone',bridge_id:'human'}}],
  ['phone.prepared',{sid:'jeff-call',connid:'callback',state:'prepared'}],
  ['phone.dialing',{sid:'jeff-call',connid:'callback',state:'dialing'}],
  ['phone.answered',{sid:'jeff-call',connid:'callback',state:'answered'}],
  ['phone.confirmed',{sid:'jeff-call',connid:'callback',state:'confirmed'}],
  ['phone.speaking',{sid:'jeff-call',connid:'callback',state:'speaking',details:{retained_connid:'jeff-phone',retired_connid:'browser',join_confirmed:true,bridge_id:'mobile'}}],
];

test('one card per person, live transport changes, private hold and read-only replay', async ({ page }) => {
  await page.route('**/v1/conference/browser/**', route => {
    const path = new URL(route.request().url()).pathname;
    return route.fulfill({json:path.endsWith('/phones') ? {phones:{owner:'+12025550190',booker:'+12025550192',organizer:'+12025550191'}} : path.endsWith('/access') ? {authenticated:true} : {authenticated:true,current:{cid:'conv_story',url:'ws://fixture',token:'fixture',worker_ready:true},conversations:[]}});
  });
  await page.route('**/uctp-client/client.mjs', route => route.fulfill({contentType:'text/javascript',body:`
    const journal=${JSON.stringify(journal)};
    const members=['owner','companion','booker','organizer','assistant'].map(role=>({participant_id:role,subject:role,role,name:{owner:'Jonathan',companion:'Andrew',booker:'Reservationist',organizer:'Jeff',assistant:'David'}[role],sip:'sip:fixture@example.invalid'}));
    window.storyFixture={count:3,mutations:0};
    export class UctpClient{
      constructor(){this.identity='owner';this.authenticated=true;}async connect(){}close(){}
      command(type,cid,payload,extra){return{type,cid,payload,...extra};}
      async request(frame){if(['message.send','session.invite','session.update','session.end'].includes(frame.type))window.storyFixture.mutations++;return{payload:{}};}
      async snapshot(_cid,cursor=0){return{state:'open',participants:members,capabilities:{sms_mode:'fake',browser_handoff:true,phone_handoff:true},events:journal.slice(0,window.storyFixture.count).map(([event_type,payload],i)=>({seq:i+1,cid:'conv_story',event_type,payload,created_at:new Date(1791900000000+i*1000).toISOString()})).filter(e=>e.seq>cursor)};}
    }
  `}));
  await page.goto('/conference/');
  await expect(page.locator('#status')).toContainText('Connected');
  await expect(page.locator('[data-person]')).toHaveCount(4);
  await expect(page.locator('[data-person=assistant]')).toHaveCount(1);
  await expect(page.locator('[data-person=owner]')).toHaveCount(1);
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Browser');
  await expect(page.locator('[data-link=owner-assistant]')).toHaveAttribute('data-transport','WebRTC');
  const geometry = await page.locator('[data-person]').evaluateAll(nodes=>nodes.map(n=>n.getAttribute('transform')));
  await page.evaluate(()=>{(window as any).storyFixture.count=18;});
  await expect(page.locator('[data-person=organizer]')).toHaveAttribute('data-status','On hold');
  await expect(page.locator('[data-link=owner-assistant]')).toHaveAttribute('data-transport','WebRTC · private');
  await page.setViewportSize({width:1440,height:1100});
  await page.screenshot({path:'test-results/conference-diagram-private.png',fullPage:true});
  await page.evaluate(()=>{(window as any).storyFixture.count=21;});
  await expect(page.locator('#network-title')).toHaveText('Calling Jonathan’s mobile');
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Browser');
  await expect(page.locator('[data-person=owner]')).toContainText('Mobile ringing…');
  await page.evaluate(()=>{(window as any).storyFixture.count=24;});
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Mobile');
  await expect(page.locator('[data-link=owner-organizer]')).toHaveAttribute('data-transport','Phone ↔ Phone');
  await expect(page.locator('[data-person=assistant]')).toHaveAttribute('data-status','Handoff complete');
  await page.locator('[data-link=owner-organizer] .link-label').click();
  await expect(page.locator('#network-detail-title')).toHaveText('Jonathan ↔ Jeff');
  await expect(page.locator('#network-detail-body')).toContainText('Phone ↔ Phone · active');
  await page.locator('#network-detail-close').click();
  expect(await page.locator('[data-person]').evaluateAll(nodes=>nodes.map(n=>n.getAttribute('transform')))).toEqual(geometry);
  await page.locator('[data-person=owner]').click();
  await expect(page.locator('#network-detail')).toBeVisible();
  await expect(page.locator('#network-detail-body')).toContainText('Mobile · Talking to Jeff');
  await page.locator('#network-detail-close').click();
  await page.locator('#story-milestones').getByRole('button',{name:'David checks with Jonathan',exact:true}).click();
  await expect(page.locator('#scene-mode')).toContainText('DIAGRAM REPLAY');
  await expect(page.locator('[data-person=organizer]')).toHaveAttribute('data-status','On hold');
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Browser');
  await page.waitForTimeout(900);
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Browser');
  await page.locator('#show-live').click();
  await expect(page.locator('[data-person=owner]')).toHaveAttribute('data-device','Mobile');
  await page.locator('#replay-play').click();
  await expect(page.locator('#replay-play')).toHaveAttribute('aria-pressed','true');
  await page.locator('#replay-play').click();
  await expect(page.locator('#replay-play')).toHaveAttribute('aria-pressed','false');
  await page.locator('#replay-position').press('Home');
  for (let i = 0; i < 17; i++) await page.locator('#replay-position').press('ArrowRight');
  await expect(page.locator('#network-title')).toContainText('David checks with Jonathan');
  await page.locator('#show-live').click();
  expect(await page.evaluate(()=>(window as any).storyFixture.mutations)).toBe(0);
  await page.screenshot({path:'test-results/conference-diagram-mobile.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});
  await expect(page.locator('#replay-play')).toBeVisible();
  await expect(page.locator('#show-live')).toBeHidden();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.screenshot({path:'test-results/conference-diagram-small-screen.png',fullPage:true});
});
