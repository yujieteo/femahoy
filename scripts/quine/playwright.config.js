// The test of the HTML files of scripts/build-quine.sh. Run it with `scripts/build-quine.sh --test`.
const { defineConfig, devices } = require('@playwright/test');

module.exports = defineConfig({
  testDir: __dirname,
  outputDir: require('path').join(process.env.FH_QUINE_OUT || '.', 'test-results'),
  timeout: 120000,
  use: { acceptDownloads: true },
  projects: [{ name: 'chromium', use: devices['Desktop Chrome'] }],
});
