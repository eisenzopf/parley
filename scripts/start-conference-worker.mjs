// Start an idle external UCTP worker. Calls begin only after an owner task arrives.
import { readFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { UctpClient } from '../clients/uctp-js/client.mjs';
import { workerEnvironment } from '../examples/conference-assistant/environment.mjs';
const [bundlePath, mode] = process.argv.slice(2);
if (!bundlePath || !['full', 'voice-only'].includes(mode)) throw new Error('Usage: node scripts/start-conference-worker.mjs provisioned.json full|voice-only');
if (!process.env.VAPI_PRIVATE_KEY) throw new Error('VAPI_PRIVATE_KEY required');
const bundle = JSON.parse(await readFile(bundlePath, 'utf8'));
const assistant = bundle.participants?.find(p => p.role === 'assistant');
if (!bundle.cid || !bundle.url || !assistant?.token || assistant.token_expires_at < Date.now() + 60000)
  throw new Error('Fresh scoped assistant credentials required; refresh the same preparation file');
if (mode === 'voice-only' && bundle.participants.some(p => p.sms)) throw new Error('Voice-only preparation must have no SMS endpoints');
const client = new UctpClient(bundle.url, assistant.token);
try {
  await client.connect();
  const { payload } = await client.request(client.command('conversation.preflight', bundle.cid));
  if (!payload.readiness?.ready_for_new_task) throw new Error('Conversation is not ready for a new task; inspect and reset before starting');
  for (const capability of ['voice', 'assistant_voice', 'browser_handoff', 'phone_handoff'])
    if (!payload.capabilities?.[capability]) throw new Error(`${capability} is unavailable`);
} finally { client.close(); }
console.log(JSON.stringify({ event: 'worker.starting', cid: bundle.cid, mode, sms: mode === 'voice-only' ? 'deferred' : 'enabled', instruction: 'Connect the owner page, then select Start coordinating when all voice participants are ready.' }));
const child = spawn(process.execPath, ['examples/conference-assistant/worker.mjs'], { stdio: 'inherit',
  env: workerEnvironment(process.env, { cid: bundle.cid, token: assistant.token, url: bundle.url, mode,
    statePath: resolve(bundlePath, '..', `worker-${mode}.json`) }) });
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => child.kill(signal));
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
