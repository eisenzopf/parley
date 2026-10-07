#!/usr/bin/env node
// Private administrator utility. All routing control goes through UCTP.
import { readFile } from 'node:fs/promises';
import { UctpClient } from '../clients/uctp-js/client.mjs';

const [action, file] = process.argv.slice(2);
if (!['list', 'resolve'].includes(action) || (action === 'resolve' && !file)) {
  throw new Error('Usage: node scripts/conference-inbox.mjs list | resolve /private/resolution.json');
}
let resolution;
if (action === 'resolve') {
  resolution = JSON.parse(await readFile(file, 'utf8'));
  if (!/^env_[A-Za-z0-9_-]{1,150}$/.test(resolution.request_id || '')
    || !Number.isSafeInteger(resolution.inbox_id) || resolution.inbox_id <= 0
    || !['conversation_id', 'participant_id', 'verification_note'].every(key => typeof resolution[key] === 'string' && resolution[key].trim())) {
    throw new Error('Resolution file requires a stable env_ request_id, positive inbox_id, conversation_id, participant_id, and verification_note');
  }
}
if (!process.env.PARLEY_API_SECRET) throw new Error('PARLEY_API_SECRET required for private inbox administration');
const client = new UctpClient(process.env.UCTP_URL || 'ws://127.0.0.1:7443', process.env.PARLEY_API_SECRET);
try {
  await client.connect();
  if (action === 'list') {
    let after = 0;
    for (;;) {
      const { payload } = await client.request(client.command('inbox.list', null, { after }));
      for (const entry of payload.entries) console.log(JSON.stringify(entry));
      if (payload.entries.length < 100) break;
      if (!Number.isSafeInteger(payload.cursor) || payload.cursor <= after) throw new Error('Inbox cursor did not advance');
      after = payload.cursor;
    }
  } else {
    const { request_id, ...payload } = resolution;
    const result = await client.request(client.command('inbox.resolve', null, payload, { id: request_id }));
    console.log(JSON.stringify(result.payload));
  }
} finally { client.close(); }
