import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BrowserConversationManager } from '../examples/conference-assistant/browser-manager.mjs';

// Uses the real HTTP/UCTP server and a real idle worker; never starts coordination.
test('browser remembers access and can create fresh conversations without token entry', async ({ page, request, context, baseURL }) => {
  const directory = await mkdtemp(join(tmpdir(), 'parley-browser-e2e-'));
  const manager = new BrowserConversationManager({ stateDir: directory,
    origin: baseURL!, httpUrl: baseURL!, uctpUrl: 'ws://127.0.0.1:17443', adminToken: 'dev-only',
    environment: { ...process.env, VAPI_PRIVATE_KEY: 'fixture-only-planning-key' }, participants: [
      { alias: 'owner', name: 'Jonathan', role: 'owner', sms: '+12025550190' },
      { alias: 'companion', name: 'Alex', role: 'companion' },
      { alias: 'booker', name: 'Travel booker', role: 'booker' },
      { alias: 'organizer', name: 'Organizer', role: 'organizer', sms: '+12025550191' },
      { alias: 'assistant', name: 'David', role: 'assistant' },
    ] });
  await manager.init();
  const server = createServer((req, res) => manager.handle(req, res)); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const endpoint = `http://127.0.0.1:${(server.address() as any).port}`;
  await page.route('**/v1/conference/browser/**', async route => {
    const incoming = new URL(route.request().url());
    const response = await route.fetch({ url: endpoint + incoming.pathname + incoming.search });
    await route.fulfill({ response });
  });
  try {
    const grant = (await (await request.post(`${endpoint}/v1/conference/browser/grants`, { headers: { authorization: 'Bearer dev-only' } })).json()).grant;
    await page.goto(`/conference/#access=${grant}`);
    await expect(page.getByRole('button', { name: 'New conversation', exact: true })).toBeEnabled();
    expect(new URL(page.url()).hash).toBe('');
    await expect(page.locator('#setup')).toBeHidden(); await expect(page.locator('#token')).toHaveValue('');
    await page.getByRole('button', { name: 'New conversation', exact: true }).click();
    await expect(page.locator('#status')).toContainText('SMS fixture');
    await expect(page.getByRole('button', { name: 'Start coordinating', exact: true })).toBeEnabled();
    const first = new URL(page.url()).searchParams.get('cid')!;
    expect(first).toMatch(/^conv_/); expect(manager.worker?.ready).toBe(true);
    expect(await page.evaluate(() => document.cookie)).not.toContain('parley_browser');
    expect((await context.cookies()).find(c => c.name === 'parley_browser')?.httpOnly).toBe(true);
    await page.reload(); await expect(page.locator('#status')).toContainText('Connected');
    await expect(page.locator('#setup')).toBeHidden();
    expect(new URL(page.url()).searchParams.get('cid')).toBe(first);
    await page.getByRole('button', { name: 'New conversation', exact: true }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('cid')).not.toBe(first);
    const second = new URL(page.url()).searchParams.get('cid')!;
    expect(manager.state.current.cid).toBe(second);
    await expect(page.getByRole('button', { name: 'Start coordinating', exact: true })).toBeEnabled();
    await expect(page.locator('#people .person')).toHaveCount(4);
    await expect(page.locator('#conversation-list option')).toHaveCount(2);
    await page.locator('#conversation-list').selectOption(first);
    await expect(page.locator('#status')).toContainText('Conversation closed');
    await expect(page.locator('#send')).toBeDisabled();
    await expect(page.getByRole('button', { name: 'New conversation', exact: true })).toBeEnabled();
    for (const conversation of manager.state.conversations) {
      const bundle = JSON.parse(await readFile(join(directory, 'conversations', conversation.cid, 'provisioned.json'), 'utf8'));
      const messages = await manager.withOwner(bundle, client => client.history(conversation.cid));
      expect(messages).toHaveLength(0);
    }
    await page.setViewportSize({ width: 1600, height: 1100 });
    await page.screenshot({ path: 'test-results/conference-managed-history.png', fullPage: true });
    await page.locator('#conversation-list').selectOption(second);
    await expect(page.locator('#status')).toContainText('Connected');
    await page.screenshot({ path: 'test-results/conference-managed-current.png', fullPage: true });
  } finally {
    await manager.stopWorker();
    if (manager.state.current) await manager.withOwner(manager.state.current, async client => {
      const b = manager.state.current;
      await client.request(client.command('conversation.close', b.cid, { verification_note: 'Browser management test finished; no communications started.' }));
    });
    await new Promise<void>(resolve => server.close(() => resolve()));
    await rm(directory, { recursive: true, force: true });
  }
});
