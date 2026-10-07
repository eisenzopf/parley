// Read-only public/static kit validation; no participant/provider credentials.
import { chromium } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const url = process.argv[2], output = process.argv[3];
if (!url || !output) throw new Error('Usage: node e2e/conference-kit.mjs <kit-url> <evidence-directory>');
await mkdir(output, { recursive: true, mode: 0o700 });
const browser = await chromium.launch({ headless: true, channel: process.env.PARLEY_BROWSER_CHANNEL === 'chromium' ? 'chromium' : 'chrome' });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  const response = await page.goto(url); assert.equal(response.status(), 200);
  assert.equal(await page.locator('h1').innerText(), 'Bring the next\nconnection.');
  const links = await page.locator('a').evaluateAll(elements => elements.map(a => a.href));
  const local = [...new Set(links.filter(link => link.startsWith(url)))];
  const checks = [], downloads = new Map();
  for (const link of local) {
    const result = await fetch(link, { signal: AbortSignal.timeout(15000) });
    assert.equal(result.status, 200, link);
    // Drain each response, including the source archive, before moving on.
    // Unread large bodies can leave a closing HTTP/1.0 connection paused.
    downloads.set(link, Buffer.from(await result.arrayBuffer()));
    checks.push({ url: link, status: result.status });
  }
  const manifest = JSON.parse(downloads.get(new URL('manifest.json', url).href).toString());
  const archive = downloads.get(new URL('parley-conference-kit.tar.gz', url).href);
  assert.equal(createHash('sha256').update(archive).digest('hex'), manifest.archive_sha256);
  const checksum = downloads.get(new URL('SHA256SUMS', url).href).toString();
  assert.ok(checksum.startsWith(manifest.archive_sha256));
  await page.screenshot({ path: `${output}/desktop.png`, fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'mobile horizontal overflow');
  await page.screenshot({ path: `${output}/mobile.png`, fullPage: true });
  assert.deepEqual(errors, []);
  await writeFile(`${output}/result.json`, JSON.stringify({ status: 'passed', url, checks,
    archive_sha256: manifest.archive_sha256, source_sha256: manifest.source_sha256,
    external_contribution_links: links.filter(link => link.startsWith('https://github.com/')),
    viewports: ['1600x1100', '390x844'], no_provider_actions: true }, null, 2), { mode: 0o600 });
  console.log('Kit assets, archive checksum and desktop/mobile layout checks passed.');
} finally { await browser.close(); }
