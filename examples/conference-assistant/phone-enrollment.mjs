import { readFile, writeFile, rename, unlink } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';

/** The signed-in demo owner has confirmed enrollment for numbers saved in setup.
 * Keep the server's admission/submission records in sync; this is an owner
 * attestation, not a claim that we queried the public signup form.
 */
export async function syncPhoneEnrollment(participants, environment, now = Date.now()) {
  const path = environment.PARLEY_SMS_ENROLLMENT_PATH;
  if (!path) return;
  const records = JSON.parse(await readFile(path, 'utf8'));
  if (records.version !== 1 || records.source !== 'https://rudeless.ai/sms'
    || records.sender_number !== (environment.PARLEY_TELNYX_FROM || environment.TELNYX_FROM_NUMBER)
    || !records.campaign_id || records.campaign_id !== environment.PARLEY_SMS_CAMPAIGN_ID
    || !Array.isArray(records.recipients)) throw new Error('SMS sender configuration does not match enrollment records');
  let changed = false;
  for (const { sms: number } of participants) {
    if (!number) continue;
    const matches = records.recipients.filter(r => r.number === number);
    // Saving a destination must not undo an explicit withdrawal.
    if (matches.some(r => r.revoked)) continue;
    if (matches.length === 1 && matches[0].web_enrollment_confirmed
      && Number.isFinite(Date.parse(matches[0].reviewed_at)) && matches[0].evidence?.trim()) continue;
    records.recipients = records.recipients.filter(r => r.number !== number);
    records.recipients.push({ number, web_enrollment_confirmed: true,
      reviewed_at: new Date(now).toISOString(), confirmation_method: 'authenticated_demo_owner',
      evidence: 'The demo owner confirmed enrollment and instructed Parley to use the numbers saved in the authenticated setup panel for the requested demo.',
      revoked: false });
    changed = true;
  }
  if (!changed) return;
  const temp = `${path}.${randomUUID()}.tmp`;
  try {
    await writeFile(temp, JSON.stringify(records, null, 2), { mode: 0o600, flag: 'wx' });
    await rename(temp, path);
  } finally { await unlink(temp).catch(error => { if (error.code !== 'ENOENT') throw error; }); }
}
