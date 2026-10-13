import { stageText } from './projection.mjs';
const NS = 'http://www.w3.org/2000/svg';
const rendered = new WeakMap();
function el(tag, attrs = {}, text) {
  const node = document.createElementNS(NS, tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = stageText(text);
  return node;
}
const positions = { owner: [30, 156], assistant: [406, 30], booker: [782, 30], organizer: [782, 286] };
const paths = {
  'owner-assistant': ['M254 221 C336 221 328 95 406 95', 325, 151],
  'assistant-booker': ['M630 95 L782 95', 706, 77],
  'assistant-organizer': ['M630 95 C728 95 686 351 782 351', 725, 228],
  'owner-organizer': ['M254 221 C425 400 602 400 782 351', 490, 359],
};
const compactPositions = { owner: [20, 30], assistant: [282, 30], booker: [282, 270], organizer: [20, 270] };
const compactPaths = {
  'owner-assistant': ['M244 95 L282 95', 264, 14],
  'assistant-booker': ['M394 160 L394 270', 394, 216],
  'assistant-organizer': ['M394 160 C394 226 132 204 132 270', 263, 239],
  'owner-organizer': ['M132 160 L132 270', 132, 216],
};
const icons = {
  Browser: 'M1 2h20v14H1z M1 6h20 M6 21h10 M11 16v5',
  Mobile: 'M6 1h10v21H6z M10 18h2',
  Phone: 'M8 2H3v5c0 8 6 14 14 14h5v-5l-6-2-2 3c-4-2-7-5-9-9l3-2z',
  'AI voice': 'M2 9v6 M7 4v16 M12 1v22 M17 4v16 M22 9v6',
};
function icon(device, x, y, size = 18) {
  const group = el('g', { transform: `translate(${x} ${y}) scale(${size / 24})`, class: 'device-icon', 'aria-hidden': 'true' });
  group.append(el('path', { d: icons[device] || icons.Phone }));
  return group;
}
function clickable(node, label, action) {
  node.setAttribute('role', 'button'); node.setAttribute('tabindex', '0'); node.setAttribute('aria-label', label);
  node.onclick = action;
  node.onkeydown = event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); action(); } };
}
export function renderNetwork(svg, network, selectedRoute, select, selectPerson = () => {}, replay = false) {
  const scene = network.scene;
  const compact = window.matchMedia('(max-width:700px)').matches;
  const signature = JSON.stringify([scene, network.edges.map(e => [e.id, e.state]), selectedRoute, replay, compact]);
  if (rendered.get(svg) === signature) return;
  rendered.set(svg, signature);
  svg.replaceChildren(); svg.setAttribute('viewBox', compact ? '0 0 530 442' : '0 0 1040 436');
  svg.classList.toggle('replaying', replay);
  svg.append(el('title', {}, scene.title), el('desc', {}, scene.description));
  const edgeState = id => network.edges.find(e => e.id === id)?.state || 'idle';
  // These layers expose the underlying transport state alongside the people view.
  const audioLayer = el('g', { class: 'connection-layer', 'data-edge': 'rtp', 'data-state': edgeState('rtp') });
  const smsLayer = el('g', { class: `message-layer ${selectedRoute === 'sms' ? 'selected' : ''}`, 'data-edge': 'sms', 'data-state': edgeState('sms') });
  svg.append(audioLayer, smsLayer);
  for (const edge of scene.links) {
    const [d, x, y] = (compact ? compactPaths : paths)[edge.id];
    const group = el('g', { 'data-link': edge.id, 'data-state': edge.state, 'data-transport': edge.label,
      class: `person-link ${edge.kind} ${edge.state} ${edge.route === selectedRoute ? 'selected' : ''}` });
    group.append(el('path', { d, class: 'link-hit' }), el('path', { d, class: 'link-line' }));
    if (edge.state === 'active') group.append(el('path', { d, class: 'audio-flow' }));
    const labelWidth = Math.max(68, edge.label.length * 6.4 + 24);
    group.append(el('rect', { x: x - labelWidth / 2, y: y - 15, width: labelWidth, height: 25, rx: 12, class: 'link-caption' }), el('text', { x, y: y + 1, 'text-anchor': 'middle', class: 'link-label' }, edge.label));
    if (edge.seq) clickable(group, `Inspect ${edge.label} between ${scene.cards.find(c => c.id === edge.from).name} and ${scene.cards.find(c => c.id === edge.to).name}`, () => select(edge.seq, edge));
    (edge.kind === 'message' ? smsLayer : audioLayer).append(group);
  }
  for (const card of scene.cards) {
    const [x, y] = (compact ? compactPositions : positions)[card.id];
    const group = el('g', { transform: `translate(${x} ${y})`, class: `person-node ${card.state} ${card.speaking ? 'speaking' : ''}`, 'data-person': card.id, 'data-device': card.device, 'data-status': card.status });
    clickable(group, `${card.name}, ${card.device}, ${card.status}. Show details`, () => selectPerson(card));
    group.append(el('rect', { width: 224, height: 130, rx: 20, class: 'person-card' }), el('circle', { cx: 32, cy: 34, r: 18, class: 'avatar' }), el('text', { x: 32, y: 40, 'text-anchor': 'middle', class: 'avatar-letter' }, card.name.charAt(0)));
    const name = stageText(card.name).split(' — ')[0];
    group.append(el('text', { x: 61, y: 33, class: 'person-name' }, name.length > 19 ? `${name.slice(0, 18)}…` : name), el('text', { x: 61, y: 51, class: 'person-role' }, { owner: 'Jonathan + Andrew’s trip', assistant: 'Jonathan’s assistant', booker: 'Travel reservationist', organizer: 'Conference organizer' }[card.id]));
    const badge = el('g', { class: 'device-badge', ...(card.id === 'owner' ? { 'data-edge': 'browser', 'data-state': edgeState('browser') } : card.id === 'assistant' ? { 'data-edge': 'vapi', 'data-state': edgeState('vapi') } : {}) });
    badge.append(el('rect', { x: 16, y: 65, width: card.device === 'AI voice' ? 99 : 96, height: 26, rx: 13 }), icon(card.device, 24, 69, 17), el('text', { x: 48, y: 83 }, card.device)); group.append(badge);
    group.append(el('circle', { cx: 22, cy: 110, r: 3.5, class: 'status-dot' }), el('text', { x: 33, y: 114, class: 'person-status' }, card.status));
    if (card.id === 'owner') {
      const pending = el('g', { 'data-edge': 'phone', 'data-state': edgeState('phone'), class: 'phone-pending', visibility: card.pending ? 'visible' : 'hidden' });
      pending.append(el('rect', { x: 0, y: 137, width: 224, height: 32, rx: 12 }), icon('Mobile', 12, 144, 17), el('text', { x: 37, y: 158 }, card.pending || ''));
      group.append(pending);
    }
    svg.append(group);
  }
  if (!scene.links.length) svg.append(el('text', { x: compact ? 263 : 519, y: compact ? 219 : 264, 'text-anchor': 'middle', class: 'idle-caption' }, compact ? 'Ready to connect' : 'Connections appear as the conversation starts'));
}
