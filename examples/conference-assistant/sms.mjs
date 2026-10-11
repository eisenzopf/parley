// Approved customer-care campaign framing applies to questions and final updates.
export function campaignSms(body) {
  if (typeof body !== 'string' || !body.trim()) throw new Error('SMS body required');
  const content = body.trim().replace(/^Rudeless Thelve:\s*/, '').replace(/\s*Reply STOP to opt out\.\s*$/, '').trim();
  if (!content) throw new Error('SMS task content required');
  return `Rudeless Thelve: ${content} Reply STOP to opt out.`;
}
