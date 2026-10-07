import { open } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { collectEvidence, EVIDENCE_MODES } from '../clients/uctp-js/evidence.mjs';
import { withOwner } from './conference-operator.mjs';

export async function saveEvidence(path, evidence) {
  // Refuse existing files and symlinks; never overwrite a prior rehearsal.
  const file = await open(path, 'wx', 0o600);
  try { await file.writeFile(`${JSON.stringify(evidence, null, 2)}\n`); }
  finally { await file.close(); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [bundle, output, mode] = process.argv.slice(2);
  try {
    if (!bundle || !output || !EVIDENCE_MODES.includes(mode) || process.argv.length !== 5) {
      throw new Error(`Usage: node scripts/export-conference-evidence.mjs provisioned.json output.json [${EVIDENCE_MODES.join('|')}]`);
    }
    await withOwner(bundle, async (client, provisioned) => {
      const evidence = await collectEvidence(client, provisioned.cid, { mode });
      await saveEvidence(output, evidence);
      console.log(JSON.stringify({ output: resolve(output), cid: evidence.cid, mode,
        events: evidence.events.length, commands: evidence.commands.length,
        unavailable: evidence.unavailable.length, publication: evidence.publication }));
    });
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
