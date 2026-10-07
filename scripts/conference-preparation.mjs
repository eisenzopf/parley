import { mkdir, readFile, writeFile, rename, open, unlink } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { UctpClient, envelope, PROFILE } from '../clients/uctp-js/client.mjs';

const roles = ['owner', 'companion', 'booker', 'organizer', 'assistant'];
async function save(path, value) {
  await writeFile(`${path}.tmp`, JSON.stringify(value, null, 2), { mode: 0o600 });
  await rename(`${path}.tmp`, path);
}
function validate(participants, httpUrl, uctpUrl) {
  if (!Array.isArray(participants) || participants.length !== roles.length || roles.some(role => participants.filter(p => p.role === role).length !== 1)) throw new Error('Exactly one owner, companion, booker, organizer and assistant required');
  const http = new URL(httpUrl), ws = new URL(uctpUrl);
  for (const url of [http, ws]) {
    if (url.username || url.password || url.search || url.hash) throw new Error('Server URLs must not contain credentials, query or fragment');
    const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname);
    if (![http === url ? 'https:' : 'wss:', ...(local ? [http === url ? 'http:' : 'ws:'] : [])].includes(url.protocol)) throw new Error('Remote preparation requires HTTPS and WSS');
  }
  return { httpUrl: http.href, uctpUrl: ws.href, participants };
}

/** Persist a create command before sending; ambiguous outcomes replay its exact ID.
 * Keep the same state file for a retry. A different file explicitly starts a new task.
 */
export async function prepareConversation({ statePath, participants, httpUrl, uctpUrl, adminToken,
  refreshTokens = false, bundleRoot = 'var/conference',
  makeClient = (url, token) => new UctpClient(url, token), fetchImpl = fetch, now = () => Date.now() }) {
  if (!adminToken) throw new Error('Administrator credential required for preparation');
  const input = validate(participants, httpUrl, uctpUrl);
  statePath = resolve(statePath);
  await mkdir(dirname(statePath), { recursive: true, mode: 0o700 });
  const lockPath = `${statePath}.lock`;
  const lock = await open(lockPath, 'wx', 0o600).catch(() => { throw new Error('Preparation lock exists; verify the previous preparation process stopped before removing it'); });
  let client;
  try {
    await lock.writeFile(String(process.pid));
    let state;
    try { state = JSON.parse(await readFile(statePath, 'utf8')); }
    catch (error) {
      if (error.code !== 'ENOENT') throw error;
      state = { version: 1, input, create: envelope('conversation.create', { participants, profile: PROFILE }), bundle: null };
      await save(statePath, state);
    }
    if (state.version !== 1 || JSON.stringify(state.input) !== JSON.stringify(input)) throw new Error('Preparation state belongs to a different roster or server; retain it and use a separate state file for a new task');
    if (!state.bundle) {
      client = makeClient(input.uctpUrl, adminToken);
      await client.connect();
      const created = await client.request(state.create);
      if (created.type !== 'conversation.opened' || !/^conv_[A-Za-z0-9_-]+$/.test(created.cid || '') || !Array.isArray(created.payload?.participants)) throw new Error('Invalid create response; retain preparation state for diagnosis/replay');
      const members = created.payload.participants;
      if (members.length !== roles.length || roles.some(role => members.filter(p => p.role === role && p.participant_id).length !== 1)) throw new Error('Created Conversation roster does not match requested roles');
      state.bundle = { cid: created.cid, url: input.uctpUrl, participants: members };
      await save(statePath, state);
    }
    const bundle = state.bundle;
    const directory = resolve(bundleRoot, bundle.cid);
    await mkdir(directory, { recursive: true, mode: 0o700 });
    const bundlePath = `${directory}/provisioned.json`;
    await save(bundlePath, bundle);
    for (const member of bundle.participants.filter(p => ['owner', 'assistant'].includes(p.role))) {
      if (!refreshTokens && member.token && member.token_expires_at > now() + 60000) continue;
      const issuedAt = now();
      const response = await fetchImpl(new URL(`/v1/conference/${encodeURIComponent(bundle.cid)}/tokens`, input.httpUrl), {
        method: 'POST', redirect: 'error', signal: AbortSignal.timeout(10000),
        headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
        body: JSON.stringify({ participant_id: member.participant_id }),
      });
      if (!response.ok) throw new Error(`Token provisioning failed (${response.status}); retry the same preparation file`);
      const value = await response.json();
      if (value.conversation_id !== bundle.cid || value.participant_id !== member.participant_id || typeof value.token !== 'string' || !value.token || !Number.isFinite(value.expires_in) || value.expires_in <= 0) throw new Error('Token response identity/expiry mismatch');
      member.token = value.token;
      member.token_expires_at = issuedAt + value.expires_in * 1000;
      await save(statePath, state);
      await save(bundlePath, bundle);
    }
    return { bundle, bundlePath, statePath };
  } finally {
    client?.close();
    await lock.close();
    await unlink(lockPath);
  }
}
