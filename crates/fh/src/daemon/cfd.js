// Optional CFD report controls. The author supplies each control in the shared UI layer.
// Data comes from --cfd, never from an inferred or fabricated force history.
async function cfdControls(text, bar, open = () => false) {
  const root = text.querySelector('[data-cfd]');
  if (!root) return {view: e => e.dataset, select: () => {}, all: () => {}};
  const inflate = async encoded => new Response(new Blob([Uint8Array.from(atob(encoded), c => c.charCodeAt(0))]).stream().pipeThrough(new DecompressionStream('deflate-raw'))).text();
  await Promise.all([...root.querySelectorAll('[data-cfd-figure]')].map(async host => {
    host.innerHTML = await inflate(host.dataset.cfdFigure);
    delete host.dataset.cfdFigure;
  }));
  const source = document.getElementById('cfd-data');
  let data = {runs: {}, ranges: {}};
  try {
    if (!source) throw new Error('No CFD source data in this report.');
    const value = JSON.parse(await inflate(source.textContent));
    if (value.version !== 1 || !value.runs || !value.ranges) throw new Error('Unsupported CFD source data.');
    data = value;
  } catch (e) {
    const note = document.createElement('p');
    note.textContent = `Shared contour limits and CFD samples are unavailable. ${e.message}`;
    root.prepend(note);
  }
  const all = selector => [...text.querySelectorAll(selector), ...bar.querySelectorAll(selector)];
  const canonical = () => bar.querySelector('[data-cfd-flap]') ?? root.querySelector('[data-cfd-flap]');
  const primary = canonical();
  all('[data-cfd-flap]').filter(s => s !== primary).forEach(s => {
    s.replaceChildren(...[...(primary?.options ?? [])].map(o => o.cloneNode(true)));
  });
  let angle = primary?.selectedOptions[0]?.dataset.angle ?? '';
  const current = () => data.runs[angle];
  let named = null, showAll = false;
  const visibility = () => all('[data-cfd-control]').forEach(e => {
    const applies = e.dataset.cfdControl !== 'cfd-force-interval' || !!current()?.history?.length;
    e.hidden = !applies || (!showAll && named !== null && !named.includes(e.dataset.cfdControl));
  });
  const option = () => [...(canonical()?.options ?? [])].find(o => o.dataset.angle === angle);
  const fmt = v => Number.isFinite(v) ? Number(v).toPrecision(6) : 'No data';
  const svgTemplate = document.createElement('template');
  svgTemplate.innerHTML = '<svg></svg>';
  const svgNamespace = svgTemplate.content.firstChild.namespaceURI;
  const svgNode = (tag, attrs = {}) => {
    const e = document.createElementNS(svgNamespace, tag);
    Object.entries(attrs).forEach(([k, v]) => e.setAttribute(k, v));
    return e;
  };
  const station = () => Number(root.querySelector('[data-cfd-station]')?.value ?? 0.8);
  const surfaceAt = points => {
    const x = station(), values = [];
    for (let i = 1; i < points.length; i++) {
      const a = points[i-1], b = points[i];
      if (x < Math.min(a[0], b[0]) || x > Math.max(a[0], b[0]) || a[0] === b[0]) continue;
      const value = a[1] + (b[1]-a[1]) * (x-a[0]) / (b[0]-a[0]);
      if (!values.some(v => Math.abs(v-value) < 1e-10)) values.push(value);
    }
    return values.map(fmt).join(', ') || 'No sample at this station';
  };
  // Use the axis labels of the emitted SVG, not a second copy of the plot scale.
  const axisX = svg => {
    const ticks = [...svg.querySelectorAll('text[text-anchor="middle"]')]
      .map(e => [Number(e.getAttribute('x')), Number(e.textContent)])
      .filter(([x,v]) => Number.isFinite(x) && Number.isFinite(v));
    if (ticks.length < 2) return null;
    const a = ticks[0], b = ticks.at(-1);
    return v => a[0] + (v-a[1]) * (b[0]-a[0]) / (b[1]-a[1]);
  };
  const drawFlow = () => {
    const host = root.querySelector('[data-cfd-flow]');
    if (!host) return;
    const run = current();
    host.replaceChildren();
    if (!run?.wall || !run?.flow) {
      host.textContent = 'No streamline data. This report has no fluid cells and velocity vectors for this control.';
      return;
    }
    const svg = svgNode('svg', {viewBox: '-0.15 -0.35 1.4 0.7', role: 'img', 'aria-label': 'Steady cell velocity near the flap. Solid line: airfoil wall. Dashed lines: streamlines.'});
    const points = path => path.map(([x,y]) => `${x},${-y}`).join(' ');
    svg.append(svgNode('polygon', {points: points(run.wall), fill: 'var(--raised)', stroke: 'var(--text)', 'stroke-width': 0.003}));
    if (root.querySelector('[data-cfd-streamlines]')?.checked) {
      const paths = [...run.flow, ...(root.querySelector('[data-cfd-seed]')?.checked ? run.flapFlow ?? [] : [])];
      for (const path of paths) {
        const line = svgNode('polyline', {points: points(path.points), fill: 'none', stroke: 'var(--accent)', style: 'stroke-width:0.002', 'stroke-dasharray': '0.012 0.006'});
        const title = svgNode('title'); title.textContent = `Trace stop: ${path.stop}`; line.append(title); svg.append(line);
        const p = path.points[0];
        svg.append(svgNode('circle', {cx:p[0], cy:-p[1], r:0.005, fill:'var(--accent)', style:'stroke:none'}));
      }
    }
    svg.append(svgNode('line', {x1:station(), x2:station(), y1:-0.25, y2:0.25, stroke:'var(--text)', 'stroke-width':0.002, 'stroke-dasharray':'0.005 0.005'}));
    svg.append(svgNode('circle', {cx:0.8, cy:-0.0044, r:0.006, fill:'var(--text)', style:'stroke:none'}));
    host.append(svg);
    const note = document.createElement('p');
    note.textContent = `Steady snapshot, not particle paths. Dots: seeds. Dashed lines: cell velocity traces. Hinge: x/c = 0.8. Source: ${run.flowSource}.`;
    host.append(note);
  };
  const update = () => {
    const run = current();
    all('[data-cfd-flap]').forEach(s => {
      const o = [...s.options].find(o => o.dataset.angle === angle);
      if (o) s.selectedIndex = o.index;
    });
    const final = run?.history?.at(-1) ?? (run?.coefficients ? [null, ...run.coefficients] : null);
    root.querySelectorAll('[data-cfd-load]').forEach(host => {
      const index = host.dataset.cfdLoad === 'lift' ? 1 : 2;
      const output = host.querySelector('output');
      if (output) output.value = final
        ? `Flap ${angle}°. Model: ${fmt(final[index])}. Reference: ${fmt(run.reference[index-1])}.`
        : 'No force coefficients in this report.';
      const svg = host.querySelector('svg');
      if (!svg) return;
      svg.querySelector('[data-cfd-cursor]')?.remove();
      svg.querySelectorAll('[data-cfd-model]').forEach(e => e.remove());
      const model = svg.querySelector('polyline');
      if (model) for (const pair of model.getAttribute('points').trim().split(/\s+/)) {
        const [x,y] = pair.split(',').map(Number);
        svg.append(svgNode('circle', {'data-cfd-model':'', cx:x, cy:y, r:4, fill:'var(--s1)'}));
      }
      const scale = axisX(svg), frame = svg.querySelector('.frame');
      if (scale && frame) svg.append(svgNode('line', {'data-cfd-cursor':'', x1:scale(Number(angle)), x2:scale(Number(angle)), y1:frame.getAttribute('y'), y2:Number(frame.getAttribute('y'))+Number(frame.getAttribute('height')), stroke:'var(--text)', 'stroke-dasharray':'4 3'}));
    });
    const cp = root.querySelector('[data-cfd-cp]');
    if (cp) {
      const output = cp.querySelector('output');
      if (output) output.value = run?.cp
        ? `Flap ${angle}°. x/c = ${station().toFixed(2)}. Upper −Cp (interpolated): ${surfaceAt(run.cp.upper)}. Lower −Cp (interpolated): ${surfaceAt(run.cp.lower)}.`
        : 'No separate upper and lower surface samples in this report.';
      const svg = cp.querySelector('svg');
      if (svg) {
        svg.querySelectorAll('polyline').forEach((p,i) => {
          const series = data.cpSeries?.[i];
          const selected = String(series?.angle) === angle;
          p.style.strokeDasharray = series?.surface === 'lower' ? '6 4' : '';
          p.style.strokeWidth = String(selected ? 3 : 1);
          p.style.opacity = selected ? '1' : '0.5';
        });
        svg.querySelectorAll('[data-cfd-cursor]').forEach(e => e.remove());
        const scale = axisX(svg), frame = svg.querySelector('.frame');
        if (scale && frame) for (const x of [0.8, station()]) svg.append(svgNode('line', {'data-cfd-cursor':'', x1:scale(x), x2:scale(x), y1:frame.getAttribute('y'), y2:Number(frame.getAttribute('y'))+Number(frame.getAttribute('height')), stroke:'var(--text)', 'stroke-dasharray':x===0.8?'6 4':'2 2'}));
      }
    }
    const history = root.querySelector('[data-cfd-history]');
    if (history) {
      history.querySelectorAll('[data-cfd-angle]').forEach(e => e.hidden = e.dataset.cfdAngle !== angle);
      const start = root.querySelector('[data-cfd-interval]');
      const rows = run?.history ?? [];
      if (start) { start.max = Math.max(0, rows.length-1); start.disabled = !rows.length; }
      const selected = rows.slice(Number(start?.value ?? 0));
      const range = index => {
        const values = selected.map(r => r[index]);
        return `${fmt(Math.min(...values))} to ${fmt(Math.max(...values))}`;
      };
      const output = history.querySelector('output');
      if (output) output.value = selected.length
        ? `Iterations ${selected[0][0]} to ${selected.at(-1)[0]}. Cl: ${range(1)}. Cm: ${range(2)}. Source: ${run.source}.`
        : 'No force history in this report.';
      const absent = history.querySelector('[data-cfd-history-absent]');
      if (absent) absent.textContent = !run?.residuals?.some(r => Object.keys(r[1]).length)
        ? 'No initial residual data in this report. Absent equations and gaps have no zero value.'
        : `Initial residuals. Source: ${run.residualSource}. Gaps have no zero value. A low residual does not prove force or mesh convergence.`;
    }
    visibility();
    drawFlow();
  };
  const refresh = () => { update(); if (option()) open(option()); };
  all('[data-cfd-field], [data-cfd-mesh], [data-cfd-station]').forEach(e => e.addEventListener('change', refresh));
  all('[data-cfd-station], [data-cfd-interval]').forEach(e => e.addEventListener('input', update));
  all('[data-cfd-streamlines], [data-cfd-seed]').forEach(e => e.addEventListener('change', drawFlow));
  const select = e => {
    const o = [...(canonical()?.options ?? [])].find(o => o.dataset.run === e.dataset.run);
    if (o) angle = o.dataset.angle;
    const controls = e.dataset.controls ?? o?.dataset.controls;
    named = controls === undefined ? null : controls.split(',').map(n => n.trim()).filter(Boolean);
    update();
  };
  all('[data-cfd-flap]').forEach(s => s.addEventListener('change', () => { if (s.selectedOptions[0]) select(s.selectedOptions[0]); }));
  if (option()) select(option()); else update();
  return {
    view(e) {
      const base = [...(canonical()?.options ?? [])].find(o => o.dataset.run === e.dataset.run);
      const settings = {...base?.dataset, ...e.dataset};
      const field = all('[data-cfd-field]').find(e => e.checked)?.value;
      if (field) {
        settings.fields = field;
        settings.quantity = field === 'p' ? 'kinematic-pressure' : 'velocity';
        if (data.ranges?.[field]) settings.range = data.ranges[field].join(',');
      }
      const mesh = all('[data-cfd-mesh]')[0];
      if (mesh) settings.mesh = String(mesh.checked);
      const selectedRun = data.runs[base?.dataset.angle ?? angle];
      if (selectedRun?.wall) {
        const hits = [];
        const wall = selectedRun.wall;
        for (let i = 1; i < wall.length; i++) {
          const a = wall[i-1], b = wall[i], x = station();
          if (x < Math.min(a[0],b[0]) || x > Math.max(a[0],b[0]) || a[0] === b[0]) continue;
          const y = a[1] + (b[1]-a[1])*(x-a[0])/(b[0]-a[0]);
          hits.push(`${x},${y},1 station x/c = ${x.toFixed(2)}`);
        }
        settings.marks = Math.abs(station()-0.8) < 0.001
          ? '0.8,0.0044,1 hinge, x/c = 0.80'
          : '0.8,0.0044,1' + (hits.length ? ';'+hits[0] : '');
      }
      return settings;
    },
    all(value) { showAll = value; visibility(); },
    select
  };
}
