import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { mkdtemp, rm, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { BrowserConversationManager } from '../examples/conference-assistant/browser-manager.mjs';
import { createPasswordVerifier } from '../examples/conference-assistant/browser-password.mjs';

// Uses the real HTTP/UCTP server and a real idle worker; never starts coordination.
test('password sign-in connects the browser and enables new conversations without token entry', async ({ page, context, baseURL }) => {
  const directory = await mkdtemp(join(tmpdir(), 'parley-browser-e2e-'));
  const manager = new BrowserConversationManager({ stateDir: directory,
    origin: baseURL!, httpUrl: baseURL!, uctpUrl: 'ws://127.0.0.1:17443', adminToken: 'dev-only',
    passwordVerifier: await createPasswordVerifier('fixture browser password'),
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
  let viewFailure = false;
  await page.route('**/v1/conference/browser/**', async route => {
    const incoming = new URL(route.request().url());
    if (viewFailure && incoming.pathname.endsWith('/session') && route.request().method() === 'GET') {
      return route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Fixture connection interruption.' }) });
    }
    const response = await route.fetch({ url: endpoint + incoming.pathname + incoming.search });
    await route.fulfill({ response });
  });
  await page.addInitScript(() => {
    const NativeSocket = window.WebSocket;
    window.WebSocket = class extends NativeSocket {
      addEventListener(type: any, listener: any, options?: any) {
        if (type !== 'message') return super.addEventListener(type, listener, options);
        return super.addEventListener(type, (event: any) => {
          let auth = false; try { auth = JSON.parse(event.data).type.startsWith('auth.'); } catch {}
          if (auth) setTimeout(() => listener.call(this, event), 900);
          else listener.call(this, event);
        }, options);
      }
    } as any;
  });
  try {
    await page.goto('/conference/#access=retired-link');
    await expect(page.locator('#password-form')).toBeVisible();
    await expect(page.locator('#new-conversation')).toBeHidden();
    await expect(page.locator('#task')).toBeHidden();
    await page.getByLabel('Password', { exact: true }).fill('wrong password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await expect(page.locator('#access-status')).toContainText('Incorrect password');
    await page.getByLabel('Password', { exact: true }).fill('fixture browser password');
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await expect(page.getByRole('button', { name: 'New conversation', exact: true })).toBeEnabled();
    expect(new URL(page.url()).hash).toBe('');
    await expect(page.locator('#setup')).toBeHidden(); await expect(page.locator('#token')).toHaveValue('');
    await expect(page.locator('#password')).toHaveValue('');
    await expect(page.getByRole('button', { name: 'Connect browser', exact: true })).toBeEnabled();
    await page.getByRole('button', { name: 'New conversation', exact: true }).click();
    await expect(page.locator('#status')).toContainText('SMS fixture');
    await expect(page.locator('#notice')).toHaveText('');
    await expect(page.getByRole('button', { name: 'Start coordinating', exact: true })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Browser connected', exact: true })).toBeVisible();
    const first = new URL(page.url()).searchParams.get('cid')!;
    expect(first).toMatch(/^conv_/); expect(manager.worker?.ready).toBe(true);
    expect(await page.evaluate(() => document.cookie)).not.toContain('parley_browser');
    expect((await context.cookies()).find(c => c.name === 'parley_browser')?.httpOnly).toBe(true);
    await page.reload(); await expect(page.locator('#status')).toContainText('Connected');
    await expect(page.locator('#setup')).toBeHidden();
    expect(new URL(page.url()).searchParams.get('cid')).toBe(first);
    // A failed conversation read must not silently disable New or remove reconnect.
    viewFailure = true; await page.reload();
    await expect(page.locator('#access-status')).toContainText('Fixture connection interruption');
    await expect(page.getByRole('button', { name: 'New conversation', exact: true })).toBeEnabled();
    await expect(page.getByRole('button', { name: 'Connect browser', exact: true })).toBeEnabled();
    viewFailure = false;
    await page.getByRole('button', { name: 'Connect browser', exact: true }).click();
    await expect(page.locator('#status')).toContainText('Connected');
    await expect(page.locator('#notice')).toHaveText('');
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
    await expect(page.locator('#notice')).toHaveText('');
    await page.screenshot({ path: 'test-results/conference-managed-current.png', fullPage: true });
    await page.getByRole('button', { name: 'Sign out', exact: true }).click();
    await expect(page.locator('#password-form')).toBeVisible();
    await expect(page.locator('#task')).toBeHidden();
    expect((await context.cookies()).some(c => c.name === 'parley_browser')).toBe(false);
    await page.reload(); await expect(page.locator('#password-form')).toBeVisible();
    expect(manager.state.current.cid).toBe(second);
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
