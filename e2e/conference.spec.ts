import { test, expect } from '@playwright/test';
import { UctpClient } from '../clients/uctp-js/client.mjs';
import { ConferenceWorker } from '../examples/conference-assistant/worker.mjs';

test('conference task, owner approval, four SMS updates, and actual worker request reveal', async ({ page, request }) => {
  const admin = new UctpClient('ws://127.0.0.1:17443', 'dev-only');
  let assistant: UctpClient | undefined;
  try {
    await admin.connect();
    const created = await admin.request(admin.command('conversation.create', null, { participants: [
      { alias: 'jonathan', name: 'Jonathan', role: 'owner', sms: '+14155550101' },
      { alias: 'alex', name: 'Alex', role: 'companion', sms: '+14155550102' },
      { alias: 'booker', name: 'Travel booker', role: 'booker', sms: '+14155550103' },
      { alias: 'organizer', name: 'Organizer', role: 'organizer', sms: '+14155550104' },
      { alias: 'assistant', name: 'Vapi assistant', role: 'assistant' },
    ] }));
    const cid = created.cid;
    const roster = created.payload.participants;
    async function token(role: string) {
      const response = await request.post(`/v1/conference/${cid}/tokens`, { headers: { authorization: 'Bearer dev-only' }, data: { participant_id: roster.find(m => m.role === role).participant_id } });
      expect(response.ok()).toBeTruthy(); return (await response.json()).token;
    }
    const ownerToken = await token('owner');
    assistant = new UctpClient('ws://127.0.0.1:17443', await token('assistant')); await assistant.connect();
    let saved: any = null;
    const storage = {
      load: async () => saved || { version: 1, cid, cursor: 0, events: [], pending: null, proposal: null, approved: null, completedProposal: null, needsDecision: false },
      save: async value => { saved = structuredClone(value); },
    };
    const worker = new ConferenceWorker({ client: assistant, cid, storage,
      planner: { decide: async context => ({ actions: context.approval
        ? [{ type: 'final_updates', proposal_id: context.approval.proposal_id, updates: context.participants.filter(m => m.role !== 'assistant').map(m => ({ to: m.participant_id, body: `${m.name}: revised arrival at terminal C, 5pm.` })) }]
        : [{ type: 'propose_arrangements', summary: 'Sandbox: alternate flight arriving at terminal C at 5pm; organizer pickup confirmed in this fixture.' }] }) },
    });
    await page.goto('/conference/');
    await page.locator('#url').fill('ws://127.0.0.1:17443');
    await page.locator('#cid').fill(cid); await page.locator('#token').fill(ownerToken);
    await page.getByRole('button', { name: 'Connect', exact: true }).click();
    await expect(page.locator('#status')).toContainText('SMS fixture');
    await expect(page.locator('#build-info')).toContainText('Rvoip 0.3.12 + conference patches');
    await expect(page.locator('#build-info')).toContainText('UCTP v1 / websocket');
    await page.getByRole('button', { name: 'Start coordinating' }).click();
    await expect.poll(async () => (await assistant!.history(cid)).length).toBe(1);
    await worker.step();
    await expect(page.locator('#proposal')).toBeVisible();
    await page.getByRole('button', { name: 'Approve these sandbox arrangements' }).click();
    await expect.poll(async () => (await assistant!.history(cid)).length).toBe(3);
    await worker.step();
    await expect(page.locator('.event').filter({ hasText: /SMS sent/ })).toHaveCount(4);
    await expect(page.locator('#mission-updates')).toHaveText('4/4 final updates sent · 0/4 delivered');
    await expect(page.locator('[data-edge=rtp]')).toHaveAttribute('data-state', 'idle');
    const smsEvent = page.locator('.event').filter({ hasText: /Vapi assistant → Organizer · sms accepted/ });
    await smsEvent.click();
    await expect(page.locator('#evidence-caption')).toContainText('Actual client request');
    await expect(page.locator('#network-title')).toContainText('Connections at event #');
    await expect(page.locator('[data-edge=sms]')).toHaveClass(/selected/);
    await expect(page.locator('#evidence')).toContainText('"type": "message.send"');
    await expect(page.locator('#evidence')).toContainText(saved.lastBatch.requests[3].id);
    await expect(page.locator('#evidence')).not.toContainText(ownerToken);
    await expect(page.locator('#people .person')).toHaveCount(4);
    await page.setViewportSize({ width: 1600, height: 1100 });
    await page.screenshot({ path: 'test-results/conference-stage.png', fullPage: true });
    await page.locator('#show-timeline').click();
    await expect(page.locator('#network-title')).toContainText('One Conversation');
    await page.locator('#community').evaluate((element: HTMLDetailsElement) => { element.open = true; });
    await expect(page.locator('#future-connectors')).toBeVisible();
    const owner = new UctpClient('ws://127.0.0.1:17443', ownerToken);
    try {
      await owner.connect();
      await owner.request(owner.command('conversation.close', cid, { verification_note: 'Local rehearsal completed.' }));
      await expect(page.locator('#status')).toContainText('Conversation closed');
      await expect(page.locator('#send')).toBeDisabled();
      await expect(page.locator('#approve')).toBeDisabled();
      await worker.step();
      expect(worker.closed).toBe(true);
    } finally { owner.close(); }

  } finally { assistant?.close(); admin.close(); }
});
