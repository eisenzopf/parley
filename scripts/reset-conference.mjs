// Scoped reset retires ONE Conversation, preserving all journal/delivery evidence.
// Provision a fresh Conversation separately after this succeeds.
import { readFile } from 'node:fs/promises';
import { withOwner } from './conference-operator.mjs';
try {
  const [bundlePath, decisionPath] = process.argv.slice(2);
  if (!bundlePath || !decisionPath) throw new Error('Usage: node scripts/reset-conference.mjs provisioned.json close-decision.json');
  const decision = JSON.parse(await readFile(decisionPath, 'utf8'));
  if (!/^env_[A-Za-z0-9_-]{1,120}$/.test(decision.request_id || '') || typeof decision.verification_note !== 'string' || !decision.verification_note.trim()) throw new Error('Decision requires a stable env_ request_id and verification_note');
  await withOwner(bundlePath, async (client, bundle) => {
    await client.request(client.command('conversation.close', bundle.cid,
      { verification_note: decision.verification_note }, { id: decision.request_id }));
    const snapshot = await client.snapshot(bundle.cid);
    if (snapshot.state !== 'closed') throw new Error('Close not verified; retain decision file and investigate');
    console.log(`Closed ${bundle.cid}; history retained and new effects disabled. Provision a fresh Conversation for another run.`);
  });
} catch (error) { console.error(`${error.message}${error.code === 409 ? '; after resolving the blocker, use a NEW decision/request ID. Keep this rejected decision for evidence.' : ''}`); process.exitCode = 1; }
