import test from 'node:test';
import assert from 'node:assert/strict';
import { checkTelnyxSms } from './preflight-telnyx-sms.mjs';

const campaign = { optinKeywords: 'START,UNSTOP', optinMessage: 'Approved resume response',
  optoutKeywords: 'STOP,STOPALL,UNSUBSCRIBE,CANCEL,END,QUIT', optoutMessage: 'Approved STOP response',
  helpKeywords: 'HELP', helpMessage: 'Approved help response' };
const keywords = [['start', 'optin'], ['stop', 'optout'], ['info', 'help']].map(([op, field]) =>
  ({ country_code: 'US', op, keywords: campaign[`${field}Keywords`].split(','), resp_text: campaign[`${field}Message`] }));
const config = { number_id: '123', phone_number: '+14155550100', profile_id: 'profile-1',
  campaign_id: 'campaign-1', webhook_url: 'https://conference.example/v1/sms/inbound' };
function fixture(overrides = {}) {
  const responses = {
    '/phone_numbers/123': { id: '123', phone_number: config.phone_number, status: 'active' },
    '/phone_numbers/123/messaging': { id: '123', phone_number: config.phone_number, messaging_profile_id: config.profile_id,
      features: { sms: { domestic_two_way: true } } },
    '/10dlc/campaign/campaign-1': { campaignId: config.campaign_id, campaignStatus: 'MNO_PROVISIONED', mock: false, ...campaign },
    '/10dlc/phone_number_campaigns/%2B14155550100': { phoneNumber: config.phone_number,
      telnyxCampaignId: config.campaign_id, assignmentStatus: 'ASSIGNED' },
    '/messaging_profiles/profile-1': { id: config.profile_id, enabled: true, webhook_url: config.webhook_url },
    '/messaging_profiles/profile-1/autoresp_configs': keywords,
    ...overrides,
  };
  responses['/10dlc/campaign/campaign-1'] = { ...campaign, ...responses['/10dlc/campaign/campaign-1'] };
  const calls = [];
  return { calls, apiKey: 'private-api-key', fetchImpl: async (url, init) => {
    calls.push({ url, init });
    assert.equal(init.method, 'GET'); assert.equal(init.redirect, 'error');
    const path = new URL(url).pathname.replace('/v2', '');
    assert.ok(Object.hasOwn(responses, path));
    const body = responses[path];
    if (body === null) return { ok: false, status: 404 };
    return { ok: true, status: 200, json: async () => ({ data: body }) };
  } };
}

test('active, approved, assigned sender is only a configuration gate, with no effects or private output', async () => {
  const f = fixture(); const result = await checkTelnyxSms(config, f);
  assert.equal(result.configuration_ready, true); assert.equal(f.calls.length, 6);
  assert.ok(result.unverified.includes('real signed delivery receipt'));
  const output = JSON.stringify(result);
  for (const value of [config.phone_number, config.campaign_id, config.webhook_url, f.apiKey]) assert.ok(!output.includes(value));
});

test('pending campaign and missing or pending assignment cannot pass, even with an active SMS number', async () => {
  for (const assignment of [null, { phoneNumber: config.phone_number, campaignId: config.campaign_id, assignmentStatus: 'PENDING_ASSIGNMENT' }]) {
    const result = await checkTelnyxSms(config, fixture({
      '/10dlc/campaign/campaign-1': { campaignId: config.campaign_id, campaignStatus: 'TCR_PENDING', mock: false },
      '/10dlc/phone_number_campaigns/%2B14155550100': assignment,
    }));
    assert.equal(result.configuration_ready, false); assert.equal(result.problems.length, 2);
  }
});

test('assigned wrong campaign, mock registration, wrong profile/callback and draft wording remain failures', async () => {
  const result = await checkTelnyxSms(config, fixture({
    '/10dlc/phone_number_campaigns/%2B14155550100': { phoneNumber: config.phone_number, telnyxCampaignId: 'other', assignmentStatus: 'ASSIGNED' },
    '/10dlc/campaign/campaign-1': { campaignId: config.campaign_id, campaignStatus: 'MNO_PROVISIONED', mock: true,
      messageFlow: 'DRAFT FOR REVIEW', helpMessage: '[SUPPORT EMAIL OR PHONE]' },
    '/phone_numbers/123/messaging': { id: '123', phone_number: config.phone_number, messaging_profile_id: 'other' },
    '/messaging_profiles/profile-1': { id: config.profile_id, enabled: true, webhook_url: 'https://other.example/' },
  }));
  assert.equal(result.configuration_ready, false); assert.ok(result.problems.length >= 5);
});

test('provider failures do not expose bodies, URLs or credentials and invalid config never sends credentials', async () => {
  const f = fixture();
  await assert.rejects(checkTelnyxSms({ ...config, webhook_url: 'http://insecure.example' }, f), /HTTPS/);
  await assert.rejects(checkTelnyxSms({ ...config, campaign_id: '../other' }, f), /campaign_id/);
  assert.equal(f.calls.length, 0);
  await assert.rejects(checkTelnyxSms(config, { apiKey: f.apiKey, fetchImpl: async () => { throw new Error('private-api-key private-body'); } }),
    error => !error.message.includes(f.apiKey) && /unverified/.test(error.message));
  await assert.rejects(checkTelnyxSms(config, { apiKey: f.apiKey, fetchImpl: async () => ({ ok: false, status: 403 }) }), /HTTP 403/);
});

test('generic or missing keyword responses cannot qualify the approved campaign', async () => {
  const result = await checkTelnyxSms(config, fixture({ '/messaging_profiles/profile-1/autoresp_configs': [] }));
  assert.equal(result.configuration_ready, false);
  assert.equal(result.keyword_configuration_matches_campaign, false);
});
