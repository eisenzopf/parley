import { test, expect } from '@playwright/test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { prepareConversation } from '../scripts/conference-preparation.mjs';
import { UctpClient } from '../clients/uctp-js/client.mjs';

test('preparation recovers a lost real UCTP create reply and partial token provisioning', async () => {
  const root = await mkdtemp(`${tmpdir()}/parley-preparation-e2e-`);
  const participants = ['owner', 'companion', 'booker', 'organizer', 'assistant'].map(role => ({ alias: role, name: role, role }));
  let createdCid, lostReply = false, failAssistant = true, createRequests = 0;
  const options = { statePath: `${root}/preparation.json`, bundleRoot: root, participants,
    httpUrl: 'http://127.0.0.1:18080', uctpUrl: 'ws://127.0.0.1:17443', adminToken: 'dev-only',
    makeClient: (url, token) => {
      const client = new UctpClient(url, token);
      return { connect: () => client.connect(), close: () => client.close(), request: async request => {
        createRequests++;
        const response = await client.request(request);
        createdCid ??= response.cid;
        if (!lostReply) { lostReply = true; throw new Error('Test loses accepted create response'); }
        return response;
      } };
    },
    fetchImpl: async (url, init) => {
      const saved = JSON.parse(await readFile(options.statePath, 'utf8'));
      const assistant = saved.bundle.participants.find(p => p.role === 'assistant');
      if (failAssistant && JSON.parse(init.body).participant_id === assistant.participant_id) return { ok: false, status: 503 };
      return fetch(url, init);
    },
  };
  let owner, assistant;
  try {
    await expect(prepareConversation(options)).rejects.toThrow('loses accepted');
    const pending = JSON.parse(await readFile(options.statePath, 'utf8'));
    expect(pending.bundle).toBeNull();
    await expect(prepareConversation(options)).rejects.toThrow('503');
    failAssistant = false;
    const result = await prepareConversation(options);
    expect(result.bundle.cid).toBe(createdCid);
    expect(createRequests).toBe(2); // same durable request twice, one Conversation
    expect(JSON.parse(await readFile(options.statePath, 'utf8')).create).toEqual(pending.create);
    const members = result.bundle.participants;
    owner = new UctpClient(options.uctpUrl, members.find(p => p.role === 'owner').token);
    assistant = new UctpClient(options.uctpUrl, members.find(p => p.role === 'assistant').token);
    await owner.connect(); await assistant.connect();
    const snapshot = await owner.snapshot(createdCid);
    expect(snapshot.participants.map(p => p.participant_id)).toEqual(members.map(p => p.participant_id));
    expect(snapshot.events.filter(e => e.event_type === 'conversation.opened')).toHaveLength(1);
    expect(snapshot.events.some(e => ['message.accepted', 'session.invited'].includes(e.event_type))).toBe(false);
    expect((await assistant.snapshot(createdCid)).participants.map(p => p.participant_id)).toEqual(members.map(p => p.participant_id));
    await owner.request(owner.command('conversation.close', createdCid, { verification_note: 'Preparation recovery test finished with no provider actions.' }));
    expect((await owner.snapshot(createdCid)).state).toBe('closed');
  } finally { owner?.close(); assistant?.close(); await rm(root, { recursive: true, force: true }); }
});
