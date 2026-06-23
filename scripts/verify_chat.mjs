// Playwright verification for Antigravity Chat dedicated sidebar view.
// Run: node scripts/verify_chat.mjs
import { chromium } from 'playwright';

const URL = 'http://127.0.0.1:8899/scripts/dashboard.html';
const OUT = 'e:/AGT_Brain/scripts/_chat_verify.png';

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 950 } });

const errors = [];
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
page.on('pageerror', e => errors.push('PAGEERROR: ' + e.message));

await page.goto(URL, { waitUntil: 'networkidle' });
await page.waitForTimeout(1500);

// Switch to chat view (sidebar dedicated view, not drawer)
await page.evaluate(() => window.switchView('chat'));
await page.waitForTimeout(3500);

const state = await page.evaluate(() => {
  const view = document.getElementById('view-chat');
  const cdpPill = document.getElementById('cv-cdp-pill');
  const inner = document.getElementById('chatv-inner');
  const input = document.getElementById('cv-input');
  const msgs = document.getElementById('chatv-msgs');
  const chips = document.getElementById('cv-chips');
  return {
    viewActive: view.classList.contains('active'),
    cdpPillClass: cdpPill ? cdpPill.className : 'MISSING',
    cdpPillText: cdpPill ? cdpPill.textContent : 'MISSING',
    msgRowCount: inner.querySelectorAll('.msg').length,
    firstMsgText: (inner.querySelector('.msg')?.textContent || '').slice(0, 80),
    inputExists: !!input,
    chipsCount: chips.querySelectorAll('.cv-chip').length,
    chatvExists: !!document.querySelector('.chatv'),
    navActive: document.querySelector('.nav-item[data-view="chat"]')?.classList.contains('active'),
  };
});

await page.screenshot({ path: OUT, fullPage: false });
console.log('=== CHAT VIEW STATE ===');
console.log(JSON.stringify(state, null, 2));
console.log('=== CONSOLE ERRORS ===');
console.log(errors.length ? errors.join('\n') : '(none)');
console.log('Screenshot:', OUT);

await browser.close();
