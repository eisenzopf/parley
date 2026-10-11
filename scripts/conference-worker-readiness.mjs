export function requireFullSms(members, payload, provider) {
  if (!provider?.configuration_ready) throw new Error('Approved sender, campaign assignment and keyword responses must pass provider preflight');
  if (payload.capabilities?.sms_mode !== 'telnyx' || !payload.capabilities?.sms_configured)
    throw new Error('Full mode requires configured Telnyx SMS');
  const recipients = ['owner', 'companion', 'booker', 'organizer'].map(role => {
    const found = members.filter(m => m.role === role);
    if (found.length !== 1) throw new Error(`${role}: one participant required`);
    if (!found[0].sms && !['owner', 'organizer'].includes(role)) return null;
    if (!/^\+1\d{10}$/.test(found[0].sms || ''))
      throw new Error(`${role}: one reviewed SMS endpoint required for the full flow`);
    if (!payload.sms_eligibility?.some(e => e.participant_id === found[0].participant_id && e.eligible === true))
      throw new Error(`${role}: web enrollment has not passed server review`);
    return found[0].sms;
  });
  const phones = recipients.filter(Boolean);
  if (new Set(phones).size !== phones.length) throw new Error('SMS endpoints must be distinct; never reuse another participant’s number');
}
