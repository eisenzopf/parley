// The assistant process needs its scoped UCTP identity and Vapi planning key.
// Carrier, SIP, host administrator and cloud credentials stay with the host.
export function workerEnvironment(source, settings) {
  const names = ['PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'TZ', 'NODE_EXTRA_CA_CERTS', 'VAPI_PRIVATE_KEY', 'VAPI_CHAT_MODEL'];
  return { ...Object.fromEntries(names.filter(name => typeof source[name] === 'string').map(name => [name, source[name]])),
    CONFERENCE_CID: settings.cid, CONFERENCE_ASSISTANT_TOKEN: settings.token,
    UCTP_URL: settings.url, CONFERENCE_DEMO_MODE: settings.mode, CONFERENCE_WORKER_STATE: settings.statePath };
}
