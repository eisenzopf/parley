// Operator-only, read-only provider configuration check. Never sends or assigns.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export async function checkTelnyxSms(config, { apiKey, fetchImpl = fetch } = {}) {
  for (const name of ['number_id', 'profile_id', 'campaign_id']) {
    if (!/^[a-zA-Z0-9-]+$/.test(config?.[name] ?? '')) throw new Error(`Valid ${name} required`);
  }
  if (!/^\+1\d{10}$/.test(config.phone_number ?? '')) throw new Error('US E.164 phone_number required');
  const callback = new URL(config.webhook_url);
  if (callback.protocol !== 'https:' || callback.username || callback.password || callback.search || callback.hash) {
    throw new Error('Plain HTTPS webhook_url required');
  }
  if (!apiKey) throw new Error('TELNYX_API_KEY or TELNYX_TEST_API_KEY required');
  async function get(path, allowMissing = false) {
    let response;
    try {
      response = await fetchImpl(`https://api.telnyx.com/v2${path}`, {
        method: 'GET', headers: { Authorization: `Bearer ${apiKey}` },
        redirect: 'error', signal: AbortSignal.timeout(15000),
      });
    } catch { throw new Error('Telnyx read failed; configuration remains unverified'); }
    if (allowMissing && response.status === 404) return null;
    if (!response.ok) throw new Error(`Telnyx read failed (HTTP ${response.status}); configuration remains unverified`);
    let body;
    try { body = await response.json(); } catch { throw new Error('Invalid Telnyx response; configuration remains unverified'); }
    return body?.data ?? body;
  }
  const [number, messaging, campaign, assignment, profile, keywords] = await Promise.all([
    get(`/phone_numbers/${config.number_id}`),
    get(`/phone_numbers/${config.number_id}/messaging`),
    get(`/10dlc/campaign/${config.campaign_id}`),
    get(`/10dlc/phone_number_campaigns/${encodeURIComponent(config.phone_number)}`, true),
    get(`/messaging_profiles/${config.profile_id}`),
    get(`/messaging_profiles/${config.profile_id}/autoresp_configs`),
  ]);
  const problems = [];
  if (number?.id !== config.number_id || number?.phone_number !== config.phone_number || number?.status !== 'active') {
    problems.push('Expected sender is not verified active');
  }
  if (messaging?.id !== config.number_id || messaging?.phone_number !== config.phone_number ||
      messaging?.messaging_profile_id !== config.profile_id || !messaging?.features?.sms?.domestic_two_way) {
    problems.push('Sender is not configured for two-way SMS on the expected profile');
  }
  if (campaign?.campaignId !== config.campaign_id || campaign?.mock !== false ||
      !['MNO_ACCEPTED', 'MNO_PROVISIONED'].includes(campaign?.campaignStatus)) {
    problems.push('Expected real campaign is not carrier approved');
  }
  if (assignment?.phoneNumber !== config.phone_number || assignment?.assignmentStatus !== 'ASSIGNED' ||
      (assignment?.telnyxCampaignId ?? assignment?.campaignId) !== config.campaign_id) {
    problems.push('Sender assignment to the expected campaign is not complete');
  }
  if (profile?.id !== config.profile_id || profile?.enabled !== true || profile?.webhook_url !== config.webhook_url) {
    problems.push('Expected enabled profile and exact signed-callback URL are not verified');
  }
  if (/DRAFT FOR REVIEW|Replace this proposed/i.test(campaign?.messageFlow ?? '') ||
      /SUPPORT EMAIL OR PHONE/i.test(campaign?.helpMessage ?? '')) {
    problems.push('Submitted campaign still contains review placeholders');
  }
  if (!Array.isArray(keywords)) throw new Error('Invalid keyword response; configuration remains unverified');
  return {
    check: 'read-only Telnyx SMS configuration; no messages or mutations',
    configuration_ready: problems.length === 0,
    campaign_status: campaign?.campaignStatus ?? 'unknown',
    assignment_status: assignment?.assignmentStatus ?? 'unassigned',
    custom_keyword_responses: keywords.length,
    problems,
    unverified: ['recipient consent', 'branded keyword responses and opt-out behavior',
      'real signed delivery receipt', 'attributed human reply in the same Conversation'],
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/preflight-telnyx-sms.mjs <private-sender-config.json>');
    const config = JSON.parse(await readFile(process.argv[2], 'utf8'));
    const result = await checkTelnyxSms(config, { apiKey: process.env.TELNYX_API_KEY || process.env.TELNYX_TEST_API_KEY });
    console.log(JSON.stringify(result, null, 2));
    if (!result.configuration_ready) process.exitCode = 1;
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
