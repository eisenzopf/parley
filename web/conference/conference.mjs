import { UctpClient } from '/uctp-client/client.mjs';
import { BrowserAudio } from '/uctp-client/browser-audio.mjs';
import { ConversationProjection, stageText } from './projection.mjs';
import { renderNetwork } from './network.mjs';
import { BrowserRingtone } from './ringtone.mjs';

const $ = id => document.getElementById(id);
let client, audio, cid, me, assistant, projection, capabilities, selectedRoute, selectedSeq, cursor = 0, activeSession = null, interruptedSession = null, proposal = null, polling = false, voiceOnly = false;
const members = new Map(); const events = []; const traces = new Map(); const channels = new Map();
const ringtone = new BrowserRingtone();
let joining = false, browserAccess = false, creatingConversation = false, workerReady = true, lastSnapshot = null, connectingConversation = false;
let loadingConversation = false, browserConnected = false;
const defaultTask = $('task').value;

const error = value => { $('notice').textContent = value?.message || String(value); };
const name = id => stageText(members.get(id)?.name || id || 'Participant');
const messageId = () => `msg_${crypto.randomUUID().replaceAll('-', '')}`;
const params = new URL(location.href).searchParams;
if (params.get('cid')) $('cid').value = params.get('cid');
if (params.get('uctp')) $('url').value = params.get('uctp');
else if (location.protocol === 'https:') $('url').value = `wss://${location.host}/uctp`;
if (params.get('mode') === 'voice-only') $('task').value = "My flight from Reno to Atlanta was canceled. Please ask the travel booker whether the reservations for Alex and me can be changed to a replacement flight with two seats. Aim to depart around 11 AM and arrive by 4 PM. This is a sandbox change only, with no real purchase. Then call the conference organizer directly to confirm pickup using the booker's actual answer. Bring me into that same call from my browser so I can move to my phone. Leave the call open until I end it, then ask me to approve the final sandbox arrangements. SMS is deferred until campaign approval; do not send any texts.";

function trace(direction, frame) {
  if (frame.type.startsWith('auth.') || frame.type.startsWith('conversation.')) return;
  const id = direction === 'send' ? frame.id : frame.in_reply_to;
  if (!id) return;
  const pair = traces.get(id) || {}; pair[direction === 'send' ? 'request' : 'response'] = frame; traces.set(id, pair);
}

async function connectConversation(access) {
  connectingConversation = true; $('conversation-list').disabled = true;
  browserConnected = false; updateNewConversationButton();
  $('notice').textContent = ''; ringtone.stop(); joining = false;
  try {
    client?.close(); audio?.closeLocal(); lastSnapshot = null; projection = null; activeSession = null; interruptedSession = null; $('recovery').classList.add('hidden'); $('join').disabled = true; $('end').disabled = true; cid = access.cid; cursor = 0; events.length = 0; traces.clear(); channels.clear();
    client = new UctpClient(access.url, access.token, { trace, timeoutMs: 20000 });
    await client.connect();
    const first = await client.snapshot(cid);
    projection = new ConversationProjection(cid, first.participants); selectedRoute = null; selectedSeq = null;
    $('timeline').replaceChildren(); $('evidence').textContent = 'No event selected.';
    proposal = null; $('proposal').classList.add('hidden');
    members.clear(); first.participants.forEach(m => members.set(m.participant_id, m));
    voiceOnly = first.participants.filter(m => m.role !== 'assistant').every(m => !m.sms);
    me = first.participants.find(m => m.subject === client.identity);
    assistant = first.participants.find(m => m.role === 'assistant');
    if (me?.role !== 'owner' || !assistant) throw new Error(browserAccess ? 'This browser could not access the conversation. Please reopen Parley.' : 'Use the owner participant token for this conference view');
    audio = new BrowserAudio(client, cid, $('remote-audio'));
    $('setup').classList.add('hidden'); $('browser-access').classList.add('hidden'); $('token').value = ''; document.body.classList.add('presenting'); $('presentation-view').textContent = 'Scroll view';
    $('status').textContent = voiceOnly ? 'Connected · Voice rehearsal · SMS deferred' : first.capabilities.sms_mode === 'fake' ? 'Connected · SMS fixture' : 'Connected · Telnyx SMS';
    $('status').className = `status ${first.capabilities.sms_mode === 'fake' ? 'fake' : 'ready'}`;
    $('assistant-state').textContent = browserAccess ? `${assistant.name} · ${workerReady ? 'ready for your task' : 'not available'}` : `${assistant.name} · waiting for external worker activity`;
    $('correlation').textContent = `CONVERSATION  ${cid}`;
    consume(first); renderPeople();
    if (!polling) { polling = true; poll(); }
  } catch (e) { browserConnected = false; client?.close(); error(e); throw e; }
  finally { connectingConversation = false; updateNewConversationButton(); }
}
$('setup').addEventListener('submit', async event => {
  event.preventDefault(); await ringtone.unlock();
  try { await connectConversation({ cid: $('cid').value.trim(), url: $('url').value.trim(), token: $('token').value }); } catch {}
});

async function poll() {
  if (connectingConversation || loadingConversation) { setTimeout(poll, 750); return; }
  const activeClient = client, activeCid = cid;
  try {
    if (activeClient && !activeClient.authenticated) {
      if (browserAccess) {
        const access = await accessRequest(`session?cid=${encodeURIComponent(activeCid)}`);
        activeClient.token = access.current.token; workerReady = access.current.worker_ready;
      }
      if (activeClient === client) await activeClient.connect();
    }
    if (activeClient) {
      const snapshot = await activeClient.snapshot(activeCid, cursor);
      if (activeClient === client && activeCid === cid) consume(snapshot);
    }
  } catch (e) {
    if (activeClient === client && activeCid === cid) {
      if (browserAccess && e.status === 401) { lockBrowser(); setTimeout(poll, 750); return; }
      if (browserAccess && [401, 403].includes(e.code)) activeClient.close();
      ringtone.stop(); $('join').disabled = true;
      $('browser-invitation').classList.add('hidden');
      browserConnected = false; updateNewConversationButton();
      $('status').textContent = 'Connection interrupted · recovering'; renderStage(false); error(e);
    }
  }
  setTimeout(poll, 750);
}

function consume(snapshot) {
  lastSnapshot = snapshot;
  browserConnected = client?.authenticated === true;
  capabilities = snapshot.capabilities;
  const needsSmsReview = !voiceOnly && capabilities.sms_enrollment_required;
  const eligible = new Set((snapshot.sms_eligibility || []).filter(e => e.eligible).map(e => e.participant_id));
  const smsReady = !needsSmsReview || (['owner', 'organizer'].every(role => snapshot.participants.some(m => m.role === role && m.sms && eligible.has(m.participant_id)))
    && snapshot.participants.filter(m => m.sms).every(m => eligible.has(m.participant_id)));
  $('send').disabled = creatingConversation || loadingConversation || snapshot.state === 'closed' || !smsReady || (browserAccess && !workerReady);
  const build = capabilities.implementation;
  $('build-info').textContent = build
    ? `Rvoip ${stageText(build.rvoip_baseline)}${build.rvoip_patched ? ' + conference patches' : ''} · ${stageText(build.profile)} · UCTP v${build.envelope_version} / ${stageText(build.control_transport)}${build.experimental ? ' · experimental' : ''}`
    : 'Server build details unavailable';
  $('status').textContent = voiceOnly ? 'Connected · Voice rehearsal · SMS deferred' : snapshot.capabilities.sms_mode === 'fake' ? 'Connected · SMS fixture' : 'Connected · Telnyx SMS';
  if (!smsReady) $('status').textContent = 'Connected · SMS enrollment review required before Start';
  for (const event of snapshot.events) {
    if (event.seq <= cursor) continue;
    if (!projection.apply(event)) continue;
    cursor = event.seq; events.push(event);
    const p = event.payload;
    if (p.from === assistant.participant_id) $('assistant-state').textContent = `${assistant.name} · activity received through UCTP`;
    if (event.event_type === 'session.invited') { activeSession = p.sid; $('end').disabled = false; }
    if (event.event_type === 'session.interrupted') {
      activeSession = p.sid; interruptedSession = p.sid; audio?.closeLocal();
      $('join').disabled = true; $('end').disabled = true;
      $('recovery').classList.remove('hidden'); $('verify-ended').disabled = false;
      $('assistant-state').textContent = 'Assistant paused · verify the interrupted call';
    }
    if (['session.ended', 'session.failed'].includes(event.event_type) && p.sid === activeSession) {
      if (interruptedSession === p.sid) $('assistant-state').textContent = `${assistant.name} · recovery verified; waiting for worker activity`;
      const ended = projection.sessions.get(p.sid);
      if (ended?.participant) channels.set(ended.participant, `Voice · ${ended.state}`);
      if (ended?.browserConnection) channels.set(me.participant_id, `WebRTC · ${ended.state}`);
      activeSession = null; interruptedSession = null; $('recovery').classList.add('hidden'); $('join').disabled = true; $('end').disabled = true; audio?.closeLocal();
    }
    if (event.event_type === 'message.accepted' && p.content_type === 'application/json') {
      try {
        const content = JSON.parse(p.body);
        if (content.type === 'travel.proposal' && p.from === assistant.participant_id) {
          proposal = content; $('proposal-title').textContent = 'Ready for your approval'; $('approve').textContent = 'Approve these sandbox arrangements'; $('proposal-text').textContent = stageText(content.summary); $('proposal').classList.remove('hidden'); $('approve').disabled = false;
        }
        if (content.type === 'travel.approval' && p.from === me.participant_id && content.approved === true && content.version === 1 && content.proposal_id === proposal?.id) { $('approve').disabled = true; $('approve').textContent = 'Approved'; $('proposal-title').textContent = 'Approved sandbox arrangements'; }
      } catch {}
    }
    if (event.event_type === 'message.accepted') for (const recipient of p.to || []) channels.set(recipient, `${p.delivery.toUpperCase()} · accepted`);
    if (event.event_type === 'message.delivery') channels.set(p.participant_id, `SMS · ${p.state}`);
    if (event.event_type === 'message.received') channels.set(p.from, 'SMS · replied');
    if (event.event_type.startsWith('connection.') && p.participant_id) channels.set(p.participant_id, `${p.transport || 'voice'} · ${p.state}`);
    if (event.event_type === 'browser.speaking') channels.set(me.participant_id, 'WebRTC ↔ telephone');
    if (event.event_type.startsWith('phone.')) {
      if (p.state === 'speaking') { channels.set(me.participant_id, 'Telephone ↔ telephone · same Conversation'); audio?.closeLocal(); }
      if (['failed', 'cancelled', 'ended'].includes(p.state)) audio?.resetPhoneMove();
    }
    addEvent(event);
  }
  const current = projection.voice;
  const invitation = snapshot.state === 'closed' ? null : projection.browserInvitation;
  $('join').disabled = joining || !capabilities.browser_handoff || !invitation;
  $('join').textContent = joining ? 'Joining organizer call…' : invitation ? 'Answer organizer call' : 'Waiting for organizer call';
  $('browser-invitation').classList.toggle('hidden', !invitation || joining);
  $('browser-invitation').textContent = invitation ? 'Organizer call ready—answer here to join Jonathan from this browser.' : '';
  ringtone.start(joining ? null : invitation?.sid);
  const phonePending = ['prepared', 'dialing', 'answered', 'confirmed', 'committing'].includes(current?.phone);
  $('move-phone').disabled = snapshot.state === 'closed' || !capabilities.phone_handoff || !me.sip || current?.browser !== 'speaking' || phonePending || current?.phone === 'unknown';
  $('cancel-phone').classList.toggle('hidden', !phonePending);
  $('cancel-phone').disabled = !phonePending || current?.phone === 'committing';
  $('phone-status').textContent = phonePending ? 'Your phone is being called. Answer and press 1 to join; your browser audio stays connected until the move succeeds.'
    : current?.phone === 'speaking' ? 'You’re on your phone. Same Conversation, same Session, organizer’s call retained.'
    : current?.phone === 'unknown' ? 'Phone move needs reconciliation. Check the actual call before retrying.'
    : ['failed', 'cancelled'].includes(current?.phone) ? 'Phone move did not complete. Browser audio retained if still available.' : '';
  if (snapshot.state === 'closed') {
    $('status').textContent = 'Conversation closed · history retained';
    for (const id of ['send', 'join', 'end', 'approve', 'verify-ended', 'move-phone', 'cancel-phone']) $(id).disabled = true;
    $('assistant-state').textContent = 'Assistant stopped · Conversation closed';
    audio?.closeLocal();
  }
  updateNewConversationButton();
  renderPeople(); renderStage(true);
}

function renderPeople() {
  $('people').replaceChildren();
  for (const m of members.values()) {
    if (m.role === 'assistant') continue;
    const card = document.createElement('div'); card.className = 'person';
    const initial = document.createElement('div'); initial.className = 'initial'; initial.textContent = m.name[0];
    const title = document.createElement('strong'); title.textContent = stageText(m.name);
    const role = document.createElement('small'); role.textContent = m.role;
    const channel = document.createElement('div'); channel.className = 'channel'; channel.textContent = channels.get(m.participant_id) || 'Not contacted yet';
    card.append(initial, title, role, channel); $('people').append(card);
  }
}

function addEvent(event) {
  $('timeline').querySelector('.empty')?.remove();
  const p = event.payload; const button = document.createElement('button'); button.className = 'event';
  let label = event.event_type.replaceAll('.', ' · ');
  if (event.event_type === 'session.speech') label = `${name(p.speaker)} · ${p.state === 'started' ? 'speaking' : 'finished speaking'}`;
  if (event.event_type === 'message.accepted') label = `${name(p.from)} → ${(p.to || []).map(name).join(', ')} · ${p.delivery} accepted`;
  if (event.event_type === 'message.delivery') label = `${name(p.participant_id)} · SMS ${p.state}`;
  if (event.event_type === 'message.received') label = `${name(p.from)} replied: ${p.body}`;
  if (event.event_type === 'session.transcript') label = `${name(p.speaker)}: ${p.text}`;
  if (event.event_type === 'browser.speaking') label = 'Jonathan joined · telephone connection retained';
  if (event.event_type === 'phone.speaking') label = 'Moved to Jonathan’s phone · same Conversation, telephone connection retained';
  button.append(document.createTextNode(stageText(label)));
  const detail = document.createElement('small'); detail.textContent = `#${event.seq} · ${new Date(event.created_at).toLocaleTimeString()} · ${event.request_id || 'inbound event'}`; button.append(detail);
  button.dataset.seq = String(event.seq);
  button.onclick = () => inspectEvent(event.seq);
  $('timeline').append(button); $('timeline').scrollTop = $('timeline').scrollHeight;
}

async function inspectEvent(seq) {
  const event = events.find(e => e.seq === seq); if (!event) return;
  $('timeline').classList.add('hidden'); $('wire-panel').classList.remove('hidden'); $('show-timeline').classList.remove('hidden');
  $('activity-title').textContent = 'One interface. See the request.';
  document.querySelectorAll('.event.selected').forEach(e => e.classList.remove('selected'));
  $('timeline').querySelector(`[data-seq="${seq}"]`)?.classList.add('selected');
  selectedSeq = seq; selectedRoute = projection.routeFor(event); renderStage(client.authenticated);
  let pair = traces.get(event.request_id);
  if (!pair && event.request_id) {
    try { pair = (await client.request(client.command('conversation.inspect', cid, { request_id: event.request_id }))).payload.evidence; }
    catch { /* Provider-originated events may have no client request. */ }
  }
  $('evidence-caption').textContent = pair?.request ? 'Actual client request, correlated reply, and durable server event.' : 'Actual event received through UCTP. This client did not originate the underlying request.';
  $('evidence').textContent = stageText(JSON.stringify({ ...(pair || {}), observed_event: event }, null, 2));
}

function renderStage(connected) {
  if (!projection) return;
  let view = projection;
  if (selectedSeq) {
    view = new ConversationProjection(cid, [...members.values()]);
    for (const event of events) { if (event.seq > selectedSeq) break; view.apply(event); }
  }
  $('network-title').textContent = selectedSeq ? `Connections at event #${selectedSeq}` : 'One Conversation, across the connections';
  $('show-live').classList.toggle('hidden', !selectedSeq);
  const network = view.network(selectedSeq ? undefined : connected, voiceOnly ? 'deferred' : capabilities?.sms_mode);
  renderNetwork($('network'), network, selectedRoute, inspectEvent);
  const voice = view.voice;
  $('network-ids').textContent = voice ? `Session ${voice.sid} · remote Connection ${voice.remote}${voice.phoneConnection ? ` · owner phone ${voice.phoneConnection}` : ''}` : 'No voice Session has been invited.';
  $('retained').textContent = network.retained ? `Same telephone Connection retained at handoff${['ended', 'failed', 'interrupted'].includes(voice.state) ? ` · Session ${voice.state}` : ''}` : 'Speaking-peer replacement will retain the existing telephone Connection.';
  $('retained').classList.toggle('proved', network.retained);
  $('mission-task').textContent = projection.task ? stageText(projection.task.payload.body.length > 180 ? `${projection.task.payload.body.slice(0, 177)}…` : projection.task.payload.body) : 'Waiting for Jonathan’s task';
  $('mission-approval').textContent = projection.approval ? 'Owner approved sandbox arrangements' : projection.proposal ? 'Waiting for owner approval' : 'Arrangements still being gathered';
  const updates = projection.finalUpdates();
  const count = states => updates.filter(u => states.includes(u.delivery?.state)).length;
  const smsTotal = [...projection.members.values()].filter(m => m.role !== 'assistant' && m.sms).length;
  const chats = updates.filter(u => u.chat && !u.member.sms).length;
  $('mission-updates').textContent = voiceOnly ? projection.voiceComplete ? 'Voice rehearsal complete · SMS deferred' : 'SMS deferred · voice rehearsal only' : `${count(['sent', 'delivered'])}/${smsTotal} final updates sent · ${count(['delivered'])}/${smsTotal} delivered${smsTotal < 4 ? ` · ${chats}/${4 - smsTotal} chat updates accepted` : ''}`;
  const uncertain = count(['unknown', 'failed']);
  $('mission-update-note').textContent = voiceOnly ? 'Text messages resume after campaign approval and delivery checks.' : uncertain ? `${uncertain} update(s) need attention` : projection.approval ? 'Sent and delivered are separate provider outcomes.' : 'Final updates wait for owner approval.';
  $('facts').replaceChildren();
  for (const fact of projection.facts.slice(-3)) {
    const item = document.createElement('button'); item.className = 'fact';
    const p = fact.payload;
    item.textContent = stageText(`${name(p.speaker || p.from)} · ${fact.event_type === 'session.transcript' ? 'final voice transcript' : 'SMS reply'}: ${p.text || p.body}`);
    item.onclick = () => inspectEvent(fact.seq); $('facts').append(item);
  }
  $('facts-empty').classList.toggle('hidden', projection.facts.length > 0);
}

async function send(body, content_type = 'text/plain') {
  return client.request(client.command('message.send', cid, { msg_id: messageId(), to: [assistant.participant_id], body, content_type, delivery: 'chat' }));
}
$('send').onclick = async () => {
  await ringtone.unlock();
  $('send').disabled = true;
  try { await send($('task').value); $('send').textContent = 'Send follow-up'; $('task-composer').open = false; }
  catch (e) { error(e); }
  finally { $('send').disabled = false; }
};
$('approve').onclick = async () => {
  if (!proposal) return; $('approve').disabled = true;
  try { await send(JSON.stringify({ type: 'travel.approval', version: 1, proposal_id: proposal.id, approved: true }), 'application/json'); }
  catch (e) { error(e); $('approve').disabled = false; }
};
$('join').onclick = async () => {
  const invitation = projection?.browserInvitation;
  if (!invitation || joining || activeSession !== invitation.sid) return;
  ringtone.answer(invitation.sid); joining = true; $('browser-invitation').classList.add('hidden'); $('join').disabled = true;
  try { await audio.join(activeSession); }
  catch (e) { error(e); }
  finally { joining = false; }
};
$('end').onclick = async () => {
  if (!activeSession) return; $('end').disabled = true;
  try { await client.request(client.command('session.end', cid, {}, { sid: activeSession })); audio.closeLocal(); }
  catch (e) { error(e); $('end').disabled = false; }
};

$('move-phone').onclick = async () => {
  $('move-phone').disabled = true;
  try { await audio.moveToPhone(); }
  catch (e) { error(e); $('move-phone').disabled = false; }
};
$('cancel-phone').onclick = async () => {
  $('cancel-phone').disabled = true;
  try { await audio.cancelPhoneMove(projection.voice?.phoneConnection); }
  catch (e) { error(e); }
};

$('verify-ended').onclick = async () => {
  if (!interruptedSession) return;
  const note = $('verification-note').value.trim();
  if (!note) { error('Describe how you verified the remote call has ended.'); return; }
  $('verify-ended').disabled = true;
  try {
    await client.request(client.command('session.update', cid,
      { kind: 'confirm_ended', verification_note: note }, { sid: interruptedSession }));
    $('verification-note').value = '';
  } catch (e) { error(e); $('verify-ended').disabled = false; }
};

$('community').addEventListener('toggle', () => { $('future-connectors').classList.toggle('hidden', !$('community').open); $('contribution-panel').classList.toggle('expanded', $('community').open); });

$('show-live').onclick = () => { selectedSeq = null; selectedRoute = null; renderStage(client?.authenticated === true); };

$('show-timeline').onclick = () => {
  $('wire-panel').classList.add('hidden'); $('timeline').classList.remove('hidden'); $('show-timeline').classList.add('hidden');
  $('activity-title').textContent = 'The Conversation, as it happens';
  selectedSeq = null; selectedRoute = null; renderStage(client?.authenticated === true);
};
renderNetwork($('network'), new ConversationProjection('', []).network(false, undefined), null, () => {});

$('presentation-view').onclick = () => { const active = document.body.classList.toggle('presenting'); $('presentation-view').textContent = active ? 'Scroll view' : 'Room view'; };


async function accessRequest(path, options = {}) {
  const response = await fetch(`/v1/conference/browser/${path}`, { credentials: 'same-origin',
    ...options, headers: { 'content-type': 'application/json', ...options.headers }, signal: AbortSignal.timeout(25000) });
  let value; try { value = await response.json(); } catch { throw new Error('The conversation service is unavailable. Please retry.'); }
  if (!response.ok) throw Object.assign(new Error(value.error || 'Could not manage this conversation.'), { status: response.status });
  return value;
}
function updateNewConversationButton() {
  const live = projection && [...projection.sessions.values()].some(s => !['ended', 'failed'].includes(s.state));
  const busy = creatingConversation || connectingConversation || loadingConversation;
  $('new-conversation').disabled = !browserAccess || busy || !!live;
  $('conversation-list').disabled = busy || !!live;
  $('sign-out').disabled = busy || !!live;
  $('connect-browser').disabled = !browserAccess || busy || browserConnected || !!live;
  $('connect-browser').textContent = connectingConversation || loadingConversation ? 'Connecting browser…' : browserConnected ? 'Browser connected' : 'Connect browser';
  $('new-conversation').title = live ? 'End the current voice call before starting a new conversation.' : '';
  $('conversation-action-status').textContent = live ? interruptedSession
    ? 'Verify the interrupted call has ended below, then you can start a new conversation.'
    : 'End the current voice call to start a new conversation.' : '';
}
function lockBrowser(message = 'Enter your password to open your conversations.') {
  browserAccess = false; browserConnected = false; client?.close(); client = null; audio?.closeLocal(); audio = null;
  ringtone.stop(); projection = null; lastSnapshot = null;
  document.body.classList.remove('presenting'); document.body.classList.add('access-locked');
  $('browser-access').classList.remove('hidden'); $('password-form').classList.remove('hidden');
  $('retry-access').classList.add('hidden'); $('sign-out').classList.add('hidden');
  $('access-title').textContent = 'Sign in to Parley'; $('access-status').textContent = message;
  $('notice').textContent = ''; $('status').textContent = 'Sign-in required'; $('status').className = 'status';
  updateNewConversationButton();
}
function unlockBrowser() {
  browserAccess = true; document.body.classList.remove('access-locked');
  $('password-form').classList.add('hidden'); $('retry-access').classList.add('hidden');
  $('sign-out').classList.remove('hidden'); $('access-title').textContent = 'Your conversations';
  $('access-status').textContent = 'Loading your conversations…'; updateNewConversationButton();
}
async function loadManagedConversation() {
  if (loadingConversation || creatingConversation || connectingConversation) return;
  loadingConversation = true; updateNewConversationButton(); $('send').disabled = true;
  const activeCid = new URL(location.href).searchParams.get('cid');
  try {
    const session = await accessRequest(activeCid ? `session?cid=${encodeURIComponent(activeCid)}` : 'session');
    if (session.pending_request_id) sessionStorage.setItem('parley.pending-new-conversation', session.pending_request_id);
    await showManagedConversation(session);
  } catch (e) {
    if (e.status === 401) { lockBrowser(); return; }
    $('browser-access').classList.remove('hidden');
    $('access-status').textContent = `${e.message} Retry loading, or select New conversation for a fresh run.`;
    $('retry-access').classList.remove('hidden'); error(e); updateNewConversationButton();
  }
  finally { loadingConversation = false; if (lastSnapshot) consume(lastSnapshot); else updateNewConversationButton(); }
}
async function showManagedConversation(session, resetTask = false) {
  browserAccess = true;
  const current = session.current;
  $('conversation-list').replaceChildren();
  for (const conversation of session.conversations || []) {
    const option = document.createElement('option'); option.value = conversation.cid;
    option.textContent = `${conversation.current ? 'Current · ' : ''}${new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }).format(new Date(conversation.created_at))}`;
    $('conversation-list').append(option);
  }
  $('conversation-list').classList.toggle('hidden', !session.conversations?.length);
  if (current) {
    workerReady = current.worker_ready;
    await connectConversation(current);
    $('conversation-list').value = current.cid;
    const url = new URL(location.href); url.searchParams.set('cid', current.cid); url.searchParams.delete('manual');
    history.replaceState(null, '', url);
    if (current.worker_failed) error('The assistant stopped. Select New conversation to start a fresh run.');
  } else {
    $('access-status').textContent = 'Your browser is ready. Select New conversation to get started.';
    $('status').textContent = 'Ready for a new conversation';
  }
  if (resetTask) { $('task').value = defaultTask; $('task-composer').open = true; $('send').textContent = 'Start coordinating'; }
  updateNewConversationButton();
}
$('new-conversation').onclick = async () => {
  await ringtone.unlock();
  creatingConversation = true; updateNewConversationButton(); $('send').disabled = true;
  $('new-conversation').textContent = 'Creating conversation…'; $('notice').textContent = '';
  const requestId = sessionStorage.getItem('parley.pending-new-conversation') || crypto.randomUUID().replaceAll('-', '');
  sessionStorage.setItem('parley.pending-new-conversation', requestId);
  try {
    const session = await accessRequest('conversations', { method: 'POST', body: JSON.stringify({ request_id: requestId }) });
    sessionStorage.removeItem('parley.pending-new-conversation');
    await showManagedConversation(session, true);
  } catch (e) {
    if (e.status === 401) lockBrowser(); else error(e);
    if (e.status === 409) sessionStorage.removeItem('parley.pending-new-conversation');
  }
  finally {
    creatingConversation = false; $('new-conversation').textContent = 'New conversation';
    if (lastSnapshot) consume(lastSnapshot); else updateNewConversationButton();
  }
};
$('conversation-list').onchange = async () => {
  try { await showManagedConversation(await accessRequest(`session?cid=${encodeURIComponent($('conversation-list').value)}`)); } catch (e) { error(e); }
};
async function enableBrowser() {
  if (params.get('manual') === '1' && ['localhost', '127.0.0.1'].includes(location.hostname)) {
    document.body.classList.remove('access-locked'); $('setup').classList.remove('hidden'); $('browser-access').classList.add('hidden'); return;
  }
  const grant = new URLSearchParams(location.hash.slice(1)).get('access');
  if (grant) history.replaceState(null, '', location.pathname + location.search);
  try {
    await accessRequest('access'); unlockBrowser(); await loadManagedConversation();
  } catch (e) {
    lockBrowser(e.status === 401 ? undefined : e.message);
    if (e.status !== 401) $('retry-access').classList.remove('hidden');
  }
}
$('password-form').addEventListener('submit', async event => {
  event.preventDefault(); $('sign-in').disabled = true; $('access-status').textContent = 'Signing in…';
  try {
    await accessRequest('session', { method: 'POST', body: JSON.stringify({ password: $('password').value }) });
    $('password').value = ''; await ringtone.unlock(); unlockBrowser(); await loadManagedConversation();
  } catch (e) { $('access-status').textContent = e.message; }
  finally { $('sign-in').disabled = false; }
});
$('retry-access').onclick = () => browserAccess ? loadManagedConversation() : enableBrowser();
$('connect-browser').onclick = () => loadManagedConversation();
$('sign-out').onclick = async () => {
  try { await accessRequest('logout', { method: 'POST' }); lockBrowser('Signed out. Enter your password to sign in again.'); }
  catch (e) { error(e); }
};

enableBrowser();

setInterval(async () => {
  if (!browserAccess || !client || creatingConversation || connectingConversation) return;
  const activeClient = client, activeCid = cid;
  try {
    const session = await accessRequest(`session?cid=${encodeURIComponent(activeCid)}`);
    if (activeClient !== client || activeCid !== cid) return;
    activeClient.token = session.current.token; workerReady = session.current.worker_ready;
    if (lastSnapshot) consume(lastSnapshot);
    if (session.current.worker_failed) error('The assistant stopped. Select New conversation to start a fresh run.');
  } catch (e) { if (activeClient === client) { if (e.status === 401) lockBrowser(); else error(e); } }
}, 60000);
