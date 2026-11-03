// femahoy README figure: how the parts of femahoy work together on your computer.
// Source of docs/figures/architecture.svg, drawn with the schematic-figure and schematic-emil skills.
// To render it: copy it to figure/figure.js of a figure project, copy style.js of schematic-emil to figure/emil.js
// and its fonts/ to figure/fonts/, then run node tools/render.mjs --root <project> --fig figure/figure.js.
// Hero: none, an architecture diagram of programs has no physical object.
// Each label comes from README.md and the diagram in section 4 of DESIGN.md.
import * as E from './emil.js';

export const figure = { width: 56, height: 108, name: 'architecture', format: 'journal' };

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
  const arrow = (a, b, kind, via) => {
    F.relate(a, b, kind);
    L.connect(F, a, b, { stroke: ink, width: line.normal, gap0: 1.5, gap1: 1.5, via, radius: m(2) });
  };

  // one column of cards, so that the figure stays narrow and its text stays legible on a phone (mm)
  const x = 3, w = 36, h = 12, cx = x + w / 2, xLoop = 48;
  const yB = 2, y1 = 25, y2 = 47, y3 = 69, y4 = 91;

  // the browser: on the computer, or on a phone over an SSH port forward
  card('browser', x, yB, 50, 11, 'Browser', 'chat and 3D viewer');

  // everything below runs on your computer
  L.rect(root, m(1), m(19), m(54), m(87), { fill: 'none', stroke: G[800], width: line.thin, dash: [2, 1.5], r: r.card });
  L.text(root, m(53), m(22), 'your computer', { anchor: 'end', valign: 'middle', size: text.small, color: G[1100] });

  card('serve', x, y1, w, h, 'fh serve', 'daemon');
  card('agent', x, y2, w, h, 'Claude Code', 'your own login', 'violet');
  card('fh', x, y3, w, h, 'fh commands', 'mesh, solve, report');
  card('state', x, y4, w, h, 'State folder', 'runs and results');

  // relations, in the direction of each action
  F.anchor('browser.foot', root, m(cx), m(yB + 11));
  arrow('browser.foot', 'serve.top', 'request');
  note(cx + 2, yB + 11 + 3, 'HTTP, 127.0.0.1:8790, token');

  arrow('serve.bottom', 'agent.top', 'starts');
  note(cx + 2, (y1 + h + y2) / 2, 'starts');

  arrow('agent.bottom', 'fh.top', 'runs');
  note(cx + 2, (y2 + h + y3) / 2, 'runs');

  arrow('fh.bottom', 'state.top', 'writes');
  note(cx + 2, (y3 + h + y4) / 2, 'writes');

  // the loop back on the right: fh serve reads the state folder
  arrow('state.right', 'serve.right', 'reads', [[m(xLoop), m(y4 + h / 2)], [m(xLoop), m(y1 + h / 2)]]);
  note(xLoop - 1.5, (y2 + h + y3) / 2, 'reads', 'end');

  E.emilTheme(F);
}
