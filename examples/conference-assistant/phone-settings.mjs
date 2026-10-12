/** Phone destinations editable by the authenticated demo owner. */
export const phoneRoles = ['owner', 'booker', 'organizer'];
export function phoneSettings(participants) {
  return Object.fromEntries(phoneRoles.map(role => {
    const member = participants.find(p => p.role === role);
    const user = member?.sip?.match(/^sips?:([^@]+)@/i)?.[1];
    return [role, user ? decodeURIComponent(user) : member?.sms || ''];
  }));
}
export function configurePhones(participants, values) {
  if (!values || typeof values !== 'object' || Array.isArray(values)
    || Object.keys(values).some(key => !phoneRoles.includes(key))) throw new Error('Provide the three participant phone numbers.');
  const normalized = Object.fromEntries(phoneRoles.map(role => {
    if (typeof values[role] !== 'string') throw new Error('Provide all three phone numbers.');
    let number = values[role].trim().replace(/[\s().-]/g, '');
    if (/^\d{10}$/.test(number)) number = '+1' + number;
    else if (/^1\d{10}$/.test(number)) number = '+' + number;
    if (!/^\+[1-9]\d{7,14}$/.test(number)) throw new Error('Use a valid phone number with country code, such as +1 415 555 0123.');
    return [role, number];
  }));
  if (normalized.owner === normalized.organizer) throw new Error('Jonathan and the organizer need distinct SMS numbers.');
  return participants.map(member => {
    if (!phoneRoles.includes(member.role)) return { ...member };
    // Keep the server-provisioned carrier route; the browser supplies only a number.
    const route = member.sip?.match(/^(sips?:)[^@]+(@[^\s]+)$/i);
    if (!route) throw new Error(`The ${member.role} calling route is not configured.`);
    const number = normalized[member.role];
    return { ...member, sip: `${route[1]}${number}${route[2]}`,
      ...(['owner', 'organizer'].includes(member.role) ? { sms: number } : {}) };
  });
}
