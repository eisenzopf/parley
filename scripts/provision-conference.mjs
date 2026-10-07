// Administration only: prepare membership/credentials; never dial or send.
import { randomUUID } from 'node:crypto';
import { resolve } from 'node:path';
import { readFile } from 'node:fs/promises';
import { prepareConversation } from './conference-preparation.mjs';

try {
  const args = process.argv.slice(2), contacts = args.shift();
  let statePath, refreshTokens = false;
  while (args.length) {
    const flag = args.shift();
    if (flag === '--state' && args[0]) statePath = args.shift();
    else if (flag === '--refresh-tokens') refreshTokens = true;
    else throw new Error('Unknown or incomplete preparation option');
  }
  if (!contacts || !process.env.PARLEY_API_SECRET) throw new Error('Usage: PARLEY_API_SECRET=... node scripts/provision-conference.mjs contacts.json [--state private-preparation.json] [--refresh-tokens]');
  statePath = resolve(statePath || `var/conference/preparations/${randomUUID()}.json`);
  console.log(`Preparation state: ${statePath}. Retry with this --state path after any failure.`);
  const result = await prepareConversation({ statePath, refreshTokens,
    participants: JSON.parse(await readFile(contacts, 'utf8')),
    httpUrl: process.env.PARLEY_HTTP_URL || 'http://127.0.0.1:8080',
    uctpUrl: process.env.UCTP_URL || 'ws://127.0.0.1:7443',
    adminToken: process.env.PARLEY_API_SECRET,
  });
  console.log(`Prepared ${result.bundle.cid}. Scoped credentials: ${result.bundlePath} (not printed).`);
  console.log('No calls or messages sent. Run preflight before starting the worker.');
} catch (error) { console.error(error.message); process.exitCode = 1; }
