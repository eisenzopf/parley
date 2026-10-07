import { test, expect } from '@playwright/test';

test('unavailable ringtone audio does not block connecting, the invitation or the owner task', async ({ page }) => {
  await page.addInitScript(() => {
    window.AudioContext = class {
      state = 'suspended';
      resume() { return new Promise(() => {}); }
    } as any;
  });
  await page.goto('/conference/');
  await page.evaluate(async () => {
    const { UctpClient } = await import('/uctp-client/client.mjs');
    const fixture = { requests: [], events: [
      { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_audio_blocked', connid: 'conn_organizer', participant_id: 'organizer' } },
      { seq: 2, event_type: 'connection.connected', payload: { sid: 'ses_audio_blocked', connid: 'conn_organizer', state: 'connected' } },
      { seq: 3, event_type: 'session.assistant_attached', payload: { sid: 'ses_audio_blocked' } },
      { seq: 4, event_type: 'message.accepted', payload: { from: 'assistant', to: ['owner'], delivery: 'chat', content_type: 'application/json',
        body: JSON.stringify({ type: 'travel.browser_invitation', version: 1, sid: 'ses_audio_blocked', retained_connid: 'conn_organizer' }) } },
    ] };
    window.pendingAudioFixture = fixture;
    UctpClient.prototype.connect = async function () { this.authenticated = true; this.identity = 'owner'; };
    UctpClient.prototype.snapshot = async function (cid, after = 0) {
      return { state: 'open', participants: [
        { participant_id: 'owner', subject: 'owner', name: 'Jonathan', role: 'owner' },
        { participant_id: 'assistant', subject: 'assistant', name: 'David', role: 'assistant' },
        { participant_id: 'organizer', subject: 'organizer', name: 'Organizer', role: 'organizer' },
      ], capabilities: { sms_mode: 'fake', browser_handoff: true }, events: fixture.events.filter(event => event.seq > after) };
    };
    UctpClient.prototype.request = async function (frame) {
      fixture.requests.push(frame); return { type: 'ack', in_reply_to: frame.id };
    };
  });
  await page.locator('#cid').fill('conv_audio_blocked');
  await page.locator('#token').fill('ui-fixture-only');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.locator('#status')).toContainText('Connected');
  await expect(page.locator('#browser-invitation')).toBeVisible();
  await expect(page.locator('#join')).toBeEnabled();
  expect(await page.evaluate(() => window.pendingAudioFixture.requests.length)).toBe(0);
  await page.locator('#task').fill('Continue the saved voice rehearsal.');
  await page.locator('#send').click();
  await expect.poll(async () => page.evaluate(() => window.pendingAudioFixture.requests.length)).toBe(1);
  expect(await page.evaluate(() => window.pendingAudioFixture.requests[0])).toMatchObject({
    type: 'message.send', payload: { to: ['assistant'], delivery: 'chat', body: 'Continue the saved voice rehearsal.' },
  });
});

// UI-only fixture. Database recovery, authorization and replay are separately
// exercised against the real UCTP host in tests/uctp_conference.rs.
test('interrupted call disables voice and records how the owner verified termination', async ({ page }) => {
  await page.goto('/conference/');
  await page.evaluate(async () => {
    const { UctpClient } = await import('/uctp-client/client.mjs');
    const fixture = {
      requests: [],
      events: [
        { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_recovery' } },
        { seq: 2, event_type: 'session.assistant_attached', payload: { sid: 'ses_recovery' } },
        { seq: 3, event_type: 'session.interrupted', payload: { sid: 'ses_recovery' } },
      ],
    };
    window.recoveryFixture = fixture;
    UctpClient.prototype.connect = async function () { this.authenticated = true; this.identity = 'owner'; };
    UctpClient.prototype.snapshot = async function (cid, after = 0) {
      return {
        participants: [
          { participant_id: 'owner', subject: 'owner', name: 'Jonathan', role: 'owner' },
          { participant_id: 'assistant', subject: 'assistant', name: 'Vapi', role: 'assistant' },
        ],
        capabilities: { sms_mode: 'fake', browser_handoff: true },
        events: fixture.events.filter(event => event.seq > after),
      };
    };
    UctpClient.prototype.request = async function (frame) {
      fixture.requests.push(frame);
      fixture.events.push({ seq: 4, event_type: 'session.ended', payload: { sid: frame.sid, source: 'owner_verification' } });
      return { type: 'ack', in_reply_to: frame.id };
    };
  });
  await page.locator('#cid').fill('conv_recovery');
  await page.locator('#token').fill('ui-fixture-only');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.locator('#recovery')).toBeVisible();
  await expect(page.locator('#assistant-state')).toContainText('Assistant paused');
  await expect(page.locator('#join')).toBeDisabled();
  await expect(page.locator('#end')).toBeDisabled();
  await page.locator('#verify-ended').click();
  await expect(page.locator('#notice')).toContainText('Describe how');
  expect(await page.evaluate(() => window.recoveryFixture.requests.length)).toBe(0);
  await page.locator('#verification-note').fill('Organizer checked their phone and confirmed the call ended.');
  await page.locator('#verify-ended').click();
  await expect(page.locator('#recovery')).toBeHidden();
  await expect(page.locator('#assistant-state')).toContainText('recovery verified');
  const frames = await page.evaluate(() => window.recoveryFixture.requests);
  expect(frames).toHaveLength(1);
  expect(frames[0]).toMatchObject({
    type: 'session.update', cid: 'conv_recovery', sid: 'ses_recovery',
    payload: { profile: 'conversation-control/1', kind: 'confirm_ended', verification_note: 'Organizer checked their phone and confirmed the call ended.' },
  });
  await expect(page.locator('#join')).toBeDisabled();
  await expect(page.locator('#end')).toBeDisabled();
});

test('organizer invitation stops ringing during control loss and stays disabled when recovery finds the call ended', async ({ page }) => {
  await page.goto('/conference/');
  await page.evaluate(async () => {
    const { UctpClient } = await import('/uctp-client/client.mjs');
    const { BrowserRingtone } = await import('/conference/ringtone.mjs');
    const fixture = { offline: false, rings: 0, events: [
      { seq: 1, event_type: 'session.invited', payload: { sid: 'ses_organizer', connid: 'conn_organizer', participant_id: 'organizer' } },
      { seq: 2, event_type: 'connection.connected', payload: { sid: 'ses_organizer', connid: 'conn_organizer', state: 'connected' } },
      { seq: 3, event_type: 'session.assistant_attached', payload: { sid: 'ses_organizer' } },
      { seq: 4, event_type: 'message.accepted', payload: { msg_id: 'msg_invite', from: 'assistant', to: ['owner'], delivery: 'chat', content_type: 'application/json',
        body: JSON.stringify({ type: 'travel.browser_invitation', version: 1, sid: 'ses_organizer', retained_connid: 'conn_organizer' }) } },
    ] };
    window.invitationRecoveryFixture = fixture;
    BrowserRingtone.prototype.beep = function () { fixture.rings++; };
    UctpClient.prototype.connect = async function () {
      if (fixture.offline) throw new Error('Fixture control connection unavailable');
      this.authenticated = true; this.identity = 'owner';
    };
    UctpClient.prototype.snapshot = async function (cid, after = 0) {
      if (fixture.offline) { this.authenticated = false; throw new Error('Fixture control connection unavailable'); }
      return { state: 'open', participants: [
        { participant_id: 'owner', subject: 'owner', name: 'Jonathan', role: 'owner' },
        { participant_id: 'assistant', subject: 'assistant', name: 'David', role: 'assistant' },
        { participant_id: 'organizer', subject: 'organizer', name: 'Organizer', role: 'organizer' },
      ], capabilities: { sms_mode: 'fake', browser_handoff: true }, events: fixture.events.filter(event => event.seq > after) };
    };
  });
  await page.locator('#cid').fill('conv_invitation_recovery');
  await page.locator('#token').fill('ui-fixture-only');
  await page.getByRole('button', { name: 'Connect', exact: true }).click();
  await expect(page.locator('#join')).toBeEnabled();
  await expect(page.locator('#browser-invitation')).toBeVisible();
  expect(await page.evaluate(() => window.invitationRecoveryFixture.rings)).toBe(1);
  await page.evaluate(() => { window.invitationRecoveryFixture.offline = true; });
  await expect(page.locator('#status')).toContainText('Connection interrupted');
  await expect(page.locator('#join')).toBeDisabled();
  await expect(page.locator('#browser-invitation')).toBeHidden();
  await page.evaluate(() => {
    const fixture = window.invitationRecoveryFixture;
    fixture.events.push({ seq: 5, event_type: 'session.ended', payload: { sid: 'ses_organizer', state: 'ended' } });
    fixture.offline = false;
  });
  await expect(page.locator('#status')).toContainText('Connected');
  await expect(page.locator('#join')).toBeDisabled();
  await expect(page.locator('#browser-invitation')).toBeHidden();
  expect(await page.evaluate(() => window.invitationRecoveryFixture.rings)).toBe(1);
});
