import { stageText } from './projection.mjs';
const NS = 'http://www.w3.org/2000/svg';
function el(tag, attrs = {}, text) {
  const node = document.createElementNS(NS, tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = stageText(text);
  return node;
}
const paths = {
  worker: ['M210 42 C340 42 380 100 470 118', 327, 31],
  owner: ['M210 241 C335 267 405 236 470 166', 340, 254],
  browser: ['M210 211 C340 211 370 163 470 149', 327, 190],
  phone: ['M210 123 L470 123', 333, 105],
  sip: ['M650 116 L950 116', 790, 98],
  rtp: ['M650 153 L950 153', 790, 174],
  vapi: ['M610 174 C680 231 728 234 820 234', 700, 224],
  sms: ['M610 98 C680 35 730 34 820 34', 695, 21],
};
export function renderNetwork(svg, network, selectedRoute, select) {
  svg.replaceChildren();
  svg.append(el('title', {}, 'Observed communication connections in this Conversation'));
  for (const edge of network.edges) {
    const [d, x, y] = paths[edge.id];
    const group = el('g', { 'data-edge': edge.id, 'data-state': edge.state, class: `network-edge ${edge.kind} ${edge.state} ${edge.route === selectedRoute ? 'selected' : ''}` });
    group.append(el('path', { d }), el('text', { x, y, 'text-anchor': 'middle' }, edge.label));
    if (edge.seq) {
      group.setAttribute('role', 'button'); group.setAttribute('tabindex', '0');
      group.setAttribute('aria-label', `Inspect ${edge.label}`);
      group.onclick = () => select(edge.seq);
      group.onkeydown = event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); select(edge.seq); } };
    }
    svg.append(group);
  }
  const nodes = [
    [35, 14, 175, 56, `${network.assistantName} · AI assistant`, 'UCTP communications client'],
    [35, 95, 175, 56, 'Jonathan’s phone', 'Callback · answer and press 1'],
    [35, 199, 175, 56, 'Jonathan’s browser', 'UCTP control + WebRTC audio'],
    [470, 98, 180, 76, 'Rvoip · Parley', 'Routes, bridges, Conversation'],
    [820, 6, 220, 56, network.smsProvider, `${network.smsRecipients} participant${network.smsRecipients === 1 ? '' : 's'} addressed individually`],
    [950, 102, 190, 62, network.target, 'Provisioned SIP / telephone route'],
    [820, 206, 220, 56, `${network.assistantName} · Vapi voice`, 'Separate from UCTP control'],
  ];
  for (const [x, y, width, height, title, subtitle] of nodes) {
    const group = el('g', { class: 'network-node' });
    const safeTitle = stageText(title);
    group.append(el('title', {}, title), el('rect', { x, y, width, height, rx: 12 }),
      el('text', { x: x + width / 2, y: y + 26, 'text-anchor': 'middle', class: 'node-title' }, safeTitle.length > 26 ? `${safeTitle.slice(0, 25)}…` : safeTitle),
      el('text', { x: x + width / 2, y: y + 45, 'text-anchor': 'middle', class: 'node-subtitle' }, subtitle));
    svg.append(group);
  }
}
