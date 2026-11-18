// Renders branding/icon.svg (icon-small.svg at 32 px, icon-16.svg at 16 px) to PNGs with
// Chromium via Playwright. Run from the repo root:
//   node branding/render-icons.js && python3 branding/make-icon-files.py
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');
const dir = __dirname;
const SIZES = [16, 32, 48, 64, 128, 256, 512, 1024];
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  fs.mkdirSync(path.join(dir, 'png'), { recursive: true });
  for (const size of SIZES) {
    const file = size <= 16 ? 'icon-16.svg' : size <= 32 ? 'icon-small.svg' : 'icon.svg';
    const svg = fs.readFileSync(path.join(dir, file), 'utf8');
    await page.setViewportSize({ width: size, height: size });
    await page.setContent(`<html><body style="margin:0;background:transparent">${svg.replace('<svg ', `<svg width="${size}" height="${size}" `)}</body></html>`);
    await page.screenshot({ path: path.join(dir, 'png', `icon-${size}.png`), omitBackground: true });
  }
  await browser.close();
  // Copies served by the editor UI (favicon, toolbar, start screen).
  for (const size of [16, 32, 256]) {
    fs.copyFileSync(path.join(dir, 'png', `icon-${size}.png`), path.join(dir, '..', 'ui', `icon-${size}.png`));
  }
})();
