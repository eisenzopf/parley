// Read-only check of the deployed closing reveal. Never authenticates or starts tasks.
import { chromium } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const output = process.argv[2];
if (!output) throw new Error('Usage: node e2e/conference-closing.mjs <private-evidence-directory>');
const stageUrl = 'https://conference.rudeless.ai/conference/';
const kitUrl = 'https://conference.rudeless.ai/kit/';
await mkdir(output, { recursive: true, mode: 0o700 });
const manifest = await (await fetch(new URL('manifest.json', kitUrl))).json();
const browser = await chromium.launch({ headless: true, channel: process.env.PARLEY_BROWSER_CHANNEL === 'chromium' ? 'chromium' : 'chrome' });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1100 }, deviceScaleFactor: 1 });
  const errors = []; page.on('pageerror', error => errors.push(error.message));
  const response = await page.goto(stageUrl);
  assert.equal(response.status(), 200);
  const htmlSha = createHash('sha256').update(await response.body()).digest('hex');
  assert.equal(htmlSha, manifest.files['web/conference/index.html']);
  await page.getByRole('button', { name: 'Room view', exact: true }).click();
  await page.locator('#community summary').click();
  const qr = page.locator('#kit-share svg');
  await qr.scrollIntoViewIfNeeded();
  assert.equal(await qr.getAttribute('role'), 'img');
  assert.equal(await page.locator('#community a').getAttribute('href'), kitUrl);
  assert.ok(await page.locator('#future-connectors').isVisible());
  const bounds = await qr.boundingBox();
  assert.ok(bounds && bounds.width >= 180 && bounds.height >= 180);
  await qr.screenshot({ path: `${output}/rendered-qr.png` });
  await page.screenshot({ path: `${output}/closing-reveal.png` });
  assert.deepEqual(errors, []);
  await writeFile(`${output}/result.json`, JSON.stringify({ status: 'passed', stage_url: stageUrl,
    kit_url: kitUrl, html_sha256: htmlSha, qr_bounds: bounds, presenter_view: true,
    no_authentication_or_provider_actions: true, qr_decoder_verified: false }, null, 2), { mode: 0o600 });
  console.log('Deployed closing reveal matches the tested source snapshot. Rendered QR ready for independent decoding.');
} finally { await browser.close(); }
