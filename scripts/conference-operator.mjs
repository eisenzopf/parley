// Operator actions use the same profile as stage and assistant clients.
import { readFile } from 'node:fs/promises';
import { UctpClient } from '../clients/uctp-js/client.mjs';

export async function withOwner(bundlePath, action) {
  const bundle = JSON.parse(await readFile(bundlePath, 'utf8'));
  const owner = bundle.participants?.find(p => p.role === 'owner');
  if (!bundle.cid || !bundle.url || !owner?.token) throw new Error('Provisioning bundle with owner token required');
  const client = new UctpClient(bundle.url, owner.token);
  try { await client.connect(); return await action(client, bundle); }
  finally { client.close(); }
}
