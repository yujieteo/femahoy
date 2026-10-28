// The HTML file of each case (scripts/build-quine.sh) opens from file://, runs the case and shows
// the pass of the check. Its .tar export holds the check and the result tables. A saved copy holds
// the results.
const { test, expect } = require('@playwright/test');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', '..');
const out = process.env.FH_QUINE_OUT;
const cases = (process.env.FH_QUINE_CASES || '').split(/\s+/).filter(Boolean);

// The result tables that the checks of a check.toml read: results/<case>/grid/<file>
// (docs/reference/check-toml.md).
function tables(id) {
  const text = fs.readFileSync(path.join(root, 'validation/cases', id, 'check.toml'), 'utf8');
  const values = (key) => [...text.matchAll(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, 'gm'))].map((m) => m[1]);
  const files = values('file');
  return values('case').map((c, i) => `results/${c}/grid/${files[i]}`);
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
    for (const table of tables(id)) expect(files[table], table).toBeTruthy();

    const saved = info.outputPath('saved.html');
    await download(page, '#quine-save', saved);
    const html = fs.readFileSync(saved, 'utf8');
    const store = JSON.parse(html.match(/<script type="application\/json" id="quine-store"[^>]*>([\s\S]*?)<\/script>/)[1]);
    const check = store.find((t) => t.title === '/home/web/results/check.txt');
    expect(check.text).toContain('1 cases: 1 pass, 0 fail');
  });
}
