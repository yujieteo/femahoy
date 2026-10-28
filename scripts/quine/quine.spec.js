// The HTML file of each case (scripts/build-quine.sh) opens from file://, runs the case and shows
// the pass of the check. The results in its .tar export agree with the references and the
// tolerance of the check.toml of the case. A saved copy holds the results.
const { test, expect } = require('@playwright/test');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', '..');
const out = process.env.FH_QUINE_OUT;
const cases = (process.env.FH_QUINE_CASES || '').split(/\s+/).filter(Boolean);

// The tolerances and the checks of a check.toml: the lines `key = value` before and in each
// `[name]` table (docs/reference/check-toml.md).
function checkToml(id) {
  const text = fs.readFileSync(path.join(root, 'validation/cases', id, 'check.toml'), 'utf8');
  const top = {};
  const checks = [];
  let at = top;
  for (const line of text.split('\n')) {
    const table = line.match(/^\[(\w+)\]/);
    const pair = line.match(/^(\w+)\s*=\s*(.+?)\s*$/);
    if (table) checks.push((at = { name: table[1] }));
    else if (pair) at[pair[1]] = JSON.parse(pair[2]);
  }
  return { relTol: top.rel_tol, absTol: top.abs_tol, checks };
}

// The value in column `name` of the last row of a table whose last comment line (or first line)
// names the columns.
function lastRow(text, name) {
  const lines = text.split('\n').filter((l) => l.trim());
  const comments = lines.filter((l) => l.startsWith('#'));
  const head = (comments.length ? comments[comments.length - 1].replace(/^#/, '') : lines.shift()).trim().split(/\s+/);
  const row = lines.filter((l) => !l.startsWith('#')).pop().trim().split(/\s+/);
  expect(head).toContain(name);
  return Number(row[head.indexOf(name)]);
}

// The entries of a ustar archive, by name.
function untar(bytes) {
  const files = {};
  for (let at = 0; at + 512 <= bytes.length;) {
    const name = bytes.subarray(at, at + 100).toString('latin1').replace(/\0.*$/s, '');
    if (!name) break;
    const prefix = bytes.subarray(at + 345, at + 500).toString('latin1').replace(/\0.*$/s, '');
    const size = parseInt(bytes.subarray(at + 124, at + 136).toString('latin1'), 8) || 0;
    files[prefix ? prefix + '/' + name : name] = bytes.subarray(at + 512, at + 512 + size).toString('utf8');
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

async function download(page, button, to) {
  const [dl] = await Promise.all([page.waitForEvent('download'), page.click(button)]);
  await dl.saveAs(to);
}

for (const id of cases) {
  test(id, async ({ page }, info) => {
    await page.goto('file://' + path.join(out, id + '.html'));
    const screen = () => page.evaluate(() => window.quine.screenText());
    await expect.poll(screen, { timeout: 100000 }).toContain('[program ');
    const text = await screen();
    expect(text).toContain('1 cases: 1 pass, 0 fail');
    expect(text).toContain('[program ended]');

    const tar = info.outputPath('files.tar');
    await download(page, '#quine-tar', tar);
    const files = untar(fs.readFileSync(tar));
    expect(files['results/check.txt']).toContain('1 cases: 1 pass, 0 fail');
    const { relTol, absTol, checks } = checkToml(id);
    for (const c of checks) {
      const table = files[`results/${c.case}/grid/${c.file}`];
      expect(table, c.name).toBeTruthy();
      const computed = lastRow(table, c.column);
      expect(Math.abs(computed - c.value), c.name).toBeLessThanOrEqual(relTol * Math.abs(c.value) + absTol);
    }

    const saved = info.outputPath('saved.html');
    await download(page, '#quine-save', saved);
    const html = fs.readFileSync(saved, 'utf8');
    const store = JSON.parse(html.match(/<script type="application\/json" id="quine-store"[^>]*>([\s\S]*?)<\/script>/)[1]);
    const check = store.find((t) => t.title === '/home/web/results/check.txt');
    expect(check.text).toContain('1 cases: 1 pass, 0 fail');
  });
}
