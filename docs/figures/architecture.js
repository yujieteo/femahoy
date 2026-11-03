// femahoy README figure: how the parts of femahoy work together on your computer.
// Source of docs/figures/architecture.svg, drawn with the schematic-figure and schematic-emil skills.
// To render it: copy it to figure/figure.js of a figure project, copy style.js of schematic-emil to figure/emil.js
// and its fonts/ to figure/fonts/, then run node tools/render.mjs --root <project> --fig figure/figure.js.
// Hero: none, an architecture diagram of programs has no physical object.
// Each label comes from README.md and the diagram in section 4 of DESIGN.md.
import * as E from './emil.js';

export const figure = { width: 72, height: 74, name: 'architecture', format: 'journal' };

export default function draw(F, L) {
  const { r, line, text } = E.useEmil(F, L, 'web');
  const m = L.mm, root = F.root, G = E.gray;
  const ink = G[900];

  // a card: the name of the part (weight 500) and 1 line of secondary text
  const card = (name, x, y, w, h, title, sub, tint) => {
    const s = tint ? { fill: E.hue[tint][3], stroke: E.hue[tint][7], r: r.card } : E.card(r.card);
    L.box(root, m(x), m(y), m(w), m(h), null, { ...s, F, name, width: line.thin });
    L.text(root, m(x + w / 2), m(y + h / 2) - 1.2, title, { anchor: 'middle', valign: 'bottom', weight: E.weight.name, size: text.label, color: G[1200] });
    L.text(root, m(x + w / 2), m(y + h / 2) + 1.4, sub, { anchor: 'middle', valign: 'top', size: text.small, color: G[1100] });
  };
  const note = (x, y, str, anchor = 'start') =>
    L.text(root, m(x), m(y), str, { anchor, valign: 'middle', size: text.small, color: G[1100] });
  const arrow = (a, b, kind) => { F.relate(a, b, kind); L.connect(F, a, b, { stroke: ink, width: line.normal, gap0: 1.5, gap1: 1.5 }); };

  // columns and rows (mm)
  const xl = 5, xr = 41, w = 26, h = 13;
  const yB = 3, y1 = 29, y2 = 55;

  // the browser: on the computer, or on a phone over an SSH port forward
  card('browser', xl, yB, 62, h, 'Browser', 'chat and 3D viewer');

  // everything below runs on your computer
  L.rect(root, m(2), m(23), m(68), m(49), { fill: 'none', stroke: G[800], width: line.thin, dash: [2, 1.5], r: r.card });
  L.text(root, m(67), m(26.5), 'your computer', { anchor: 'end', valign: 'middle', size: text.small, color: G[1100] });

  card('serve', xl, y1, w, h, 'fh serve', 'daemon');
  card('agent', xr, y1, w, h, 'Claude Code', 'your own login', 'violet');
  card('state', xl, y2, w, h, 'State folder', 'runs and results');
  card('fh', xr, y2, w, h, 'fh commands', 'mesh, solve, report');

  // relations, in the direction of each action
  const browserFoot = [m(xl + w / 2), m(yB + h)];
  F.anchor('browser.foot', root, ...browserFoot);
  arrow('browser.foot', 'serve.top', 'request');
  note(xl + w / 2 + 2, yB + h + 3.3, 'HTTP, 127.0.0.1:8790, token');

  arrow('serve.right', 'agent.left', 'starts');
  note((xl + w + xr) / 2, y1 + h / 2 - 2.6, 'starts', 'middle');

  arrow('agent.bottom', 'fh.top', 'runs');
  note(xr + w / 2 + 2, (y1 + h + y2) / 2, 'runs');

  arrow('fh.left', 'state.right', 'writes');
  note((xl + w + xr) / 2, y2 + h / 2 - 2.6, 'writes', 'middle');

  arrow('state.top', 'serve.bottom', 'reads');
  note(xl + w / 2 - 2, (y1 + h + y2) / 2, 'reads', 'end');

  E.emilTheme(F);
}
