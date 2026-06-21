/**
 * Grok Cookie Refresh v2 — CDP-based (Chrome DevTools Protocol)
 * 
 * Strategy: Launch Bố's REAL Edge browser with remote debugging,
 * then extract cookies via CDP. No Cloudflare issues because
 * it's the actual browser, not automation.
 * 
 * Flow:
 *   1. --login:   Launch Edge with CDP enabled → Bố navigates to grok.com → login
 *   2. --extract:  Connect to running Edge via CDP → extract grok.com cookies → push to grok2api
 *   3. --status:   Check current grok2api token status
 *   4. --auto:     Full auto: launch Edge → extract → push → close (needs existing session)
 * 
 * Usage:
 *   node cookie_refresh_v2.js --login     Open Edge, login to grok.com
 *   node cookie_refresh_v2.js --extract   Extract cookie from running Edge & push to grok2api
 *   node cookie_refresh_v2.js --status    Check grok2api tokens
 *   node cookie_refresh_v2.js --auto      Full auto refresh (Edge must have saved session)
 */

const http = require('http');
const https = require('https');
const { exec, execSync } = require('child_process');
const path = require('path');
const fs = require('fs');

// ═══════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════

const CONFIG = {
  GROK2API_URL: process.env.GROK2API_URL || 'http://127.0.0.1:8000',
  GROK2API_ADMIN_KEY: process.env.GROK2API_ADMIN_KEY || 'grok2api',
  
  // CDP port for Edge
  CDP_PORT: 9515,
  
  // Edge executable path
  EDGE_PATH: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  
  // Separate user data dir (won't interfere with main Edge)
  EDGE_USER_DATA: path.join(__dirname, 'edge_profile'),
  
  // Cookie target
  GROK_DOMAIN: '.grok.com',
  GROK_URL: 'https://grok.com',
  
  // Log
  LOG_FILE: path.join(__dirname, 'cookie_refresh.log'),
};

// Check both 64-bit and 32-bit paths
if (!fs.existsSync(CONFIG.EDGE_PATH)) {
  CONFIG.EDGE_PATH = 'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe';
}

// ═══════════════════════════════════════════
// Utility
// ═══════════════════════════════════════════

function log(emoji, msg) {
  const ts = new Date().toLocaleString('vi-VN', { timeZone: 'Asia/Ho_Chi_Minh' });
  const line = `[${ts}] ${emoji} ${msg}`;
  console.log(line);
  fs.appendFileSync(CONFIG.LOG_FILE, line + '\n');
}

function httpReq(method, urlStr, body = null) {
  return new Promise((resolve, reject) => {
    const url = new URL(urlStr);
    const client = url.protocol === 'https:' ? https : http;
    const options = {
      hostname: url.hostname,
      port: url.port,
      path: url.pathname + url.search,
      method,
      headers: {
        'Content-Type': 'application/json',
      },
      timeout: 10000,
    };
    
    // Add admin auth for grok2api
    if (urlStr.includes(CONFIG.GROK2API_URL)) {
      options.headers['Authorization'] = `Bearer ${CONFIG.GROK2API_ADMIN_KEY}`;
    }

    const req = client.request(options, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try { resolve({ status: res.statusCode, data: JSON.parse(data) }); }
        catch { resolve({ status: res.statusCode, data }); }
      });
    });
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('timeout')); });
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

// ═══════════════════════════════════════════
// CDP Functions
// ═══════════════════════════════════════════

/**
 * Get CDP WebSocket URL from /json endpoint
 */
async function getCdpTargets() {
  const resp = await httpReq('GET', `http://127.0.0.1:${CONFIG.CDP_PORT}/json`);
  if (resp.status !== 200) throw new Error(`CDP /json failed: ${resp.status}`);
  return resp.data;
}

/**
 * Send CDP command via WebSocket and get response
 */
function cdpCommand(wsUrl, method, params = {}) {
  return new Promise((resolve, reject) => {
    // Dynamic import for WebSocket
    const WebSocket = require('ws');
    const ws = new WebSocket(wsUrl, {
      handshakeTimeout: 5000,
      perMessageDeflate: false,
    });
    const id = 1;
    
    ws.on('open', () => {
      ws.send(JSON.stringify({ id, method, params }));
    });
    
    ws.on('message', (data) => {
      try {
        const msg = JSON.parse(data.toString());
        if (msg.id === id) {
          ws.close();
          if (msg.error) reject(new Error(JSON.stringify(msg.error)));
          else resolve(msg.result);
        }
      } catch (e) {}
    });
    
    ws.on('error', reject);
    setTimeout(() => { ws.close(); reject(new Error('CDP timeout')); }, 15000);
  });
}

async function getBrowserCdpUrl() {
  const resp = await httpReq('GET', `http://127.0.0.1:${CONFIG.CDP_PORT}/json/version`);
  if (resp.status !== 200) throw new Error(`CDP /json/version failed: ${resp.status}`);
  return resp.data.webSocketDebuggerUrl;
}

/**
 * Extract cookies from a CDP target for grok.com domain
 */
async function extractCookiesViaCdp() {
  log('🔌', `Connecting to Edge Browser CDP on port ${CONFIG.CDP_PORT}...`);
  
  const browserWsUrl = await getBrowserCdpUrl();
  if (!browserWsUrl) {
    throw new Error('No browser WebSocket URL found. Is Edge running with --remote-debugging-port?');
  }
  
  log('🎯', `Using browser WebSocket: ${browserWsUrl}`);
  
  // Get all cookies from the browser context
  const result = await cdpCommand(browserWsUrl, 'Storage.getCookies', {});
  const allCookies = result.cookies || [];
  
  // Filter grok.com and x.ai cookies
  const cookies = allCookies.filter(c => 
    c.domain && (c.domain.includes('grok.com') || c.domain.includes('x.ai'))
  );
  
  log('🍪', `Retrieved ${cookies.length} cookies for grok.com/x.ai out of ${allCookies.length} total browser cookies`);
  
  return cookies;
}

// ═══════════════════════════════════════════
// Core Flows
// ═══════════════════════════════════════════

/**
 * Launch Edge with CDP enabled and navigate to grok.com
 */
async function loginFlow() {
  log('🚀', 'Launching Edge browser for grok.com login...');
  
  // Kill any existing Edge with our debug port
  try { execSync('taskkill /F /IM msedge.exe 2>nul', { stdio: 'ignore' }); } catch {}
  await sleep(1000);
  
  const args = [
    `"${CONFIG.EDGE_PATH}"`,
    `--remote-debugging-port=${CONFIG.CDP_PORT}`,
    `--user-data-dir="${CONFIG.EDGE_USER_DATA}"`,
    '--no-first-run',
    '--no-default-browser-check',
    CONFIG.GROK_URL,
  ].join(' ');
  
  log('📌', `Edge profile: ${CONFIG.EDGE_USER_DATA}`);
  log('📌', `CDP port: ${CONFIG.CDP_PORT}`);
  
  exec(args, { detached: true, stdio: 'ignore' });
  
  log('✅', 'Edge launched! Please login to grok.com in the browser.');
  log('💡', 'After login, run: node cookie_refresh_v2.js --extract');
  log('⚠️', 'Do NOT close Edge until extraction is done.');
}

/**
 * Extract cookies from running Edge and push to grok2api
 */
async function extractFlow() {
  log('🔄', 'Extracting grok.com cookies from Edge via CDP...');
  
  try {
    const cookies = await extractCookiesViaCdp();
    
    if (cookies.length === 0) {
      log('❌', 'No cookies found. Make sure you are logged into grok.com in Edge.');
      return;
    }
    
    // Look for SSO-related cookies
    const ssoCookies = cookies.filter(c => 
      c.name === 'sso' || 
      c.name === 'sso_rw' || 
      c.name.startsWith('sso') ||
      c.name.includes('auth') ||
      c.name.includes('session') ||
      c.name.includes('token')
    );
    
    log('🔑', `Found ${ssoCookies.length} auth-related cookies:`);
    for (const c of ssoCookies) {
      log('  🍪', `${c.name} = ${c.value.substring(0, 20)}... (domain: ${c.domain}, expires: ${c.expires > 0 ? new Date(c.expires * 1000).toISOString() : 'session'})`);
    }
    
    // Build the SSO token string
    // grok2api expects the cookie value from 'sso' or 'sso_rw' cookie
    let ssoToken = null;
    
    // Priority: sso_rw > sso > any sso* cookie
    const ssoRw = cookies.find(c => c.name === 'sso_rw');
    const sso = cookies.find(c => c.name === 'sso');
    const ssoAny = cookies.find(c => c.name.startsWith('sso'));
    
    if (ssoRw) {
      ssoToken = ssoRw.value;
      log('✅', `Using sso_rw cookie (${ssoToken.substring(0, 16)}...)`);
    } else if (sso) {
      ssoToken = sso.value;
      log('✅', `Using sso cookie (${ssoToken.substring(0, 16)}...)`);
    } else if (ssoAny) {
      ssoToken = ssoAny.value;
      log('✅', `Using ${ssoAny.name} cookie (${ssoToken.substring(0, 16)}...)`);
    }
    
    if (!ssoToken) {
      // Fallback: dump all cookies for manual inspection
      log('⚠️', 'No SSO cookie found. All cookies:');
      for (const c of cookies) {
        log('  🍪', `${c.name} = ${c.value.substring(0, 30)}...`);
      }
      log('💡', 'If you see the right cookie above, let me know its name.');
      return;
    }
    
    // Save cookie locally as backup
    const backupPath = path.join(__dirname, 'last_sso_cookie.txt');
    fs.writeFileSync(backupPath, ssoToken);
    log('💾', `Cookie backed up to ${backupPath}`);
    
    // Push to grok2api
    await pushToGrok2Api(ssoToken);
    
  } catch (err) {
    if (err.message.includes('ECONNREFUSED')) {
      log('❌', `Cannot connect to Edge CDP on port ${CONFIG.CDP_PORT}.`);
      log('💡', 'Run "node cookie_refresh_v2.js --login" first to launch Edge with CDP.');
    } else {
      log('❌', `Error: ${err.message}`);
    }
  }
}

/**
 * Full auto: launch Edge (background), wait, extract, push
 */
async function autoFlow() {
  log('🤖', 'Full auto-refresh flow starting...');
  
  // Check if Edge with CDP is already running
  let cdpRunning = false;
  try {
    await getCdpTargets();
    cdpRunning = true;
    log('✅', 'Edge CDP already running');
  } catch {
    log('🚀', 'Launching Edge with CDP...');
    
    const args = [
      `"${CONFIG.EDGE_PATH}"`,
      `--remote-debugging-port=${CONFIG.CDP_PORT}`,
      `--user-data-dir="${CONFIG.EDGE_USER_DATA}"`,
      '--headless=new',
      '--no-first-run',
      '--disable-gpu',
      CONFIG.GROK_URL,
    ].join(' ');
    
    exec(args, { detached: true, stdio: 'ignore' });
    
    // Wait for Edge to start
    for (let i = 0; i < 10; i++) {
      await sleep(2000);
      try {
        await getCdpTargets();
        cdpRunning = true;
        log('✅', 'Edge CDP ready');
        break;
      } catch {}
    }
  }
  
  if (!cdpRunning) {
    log('❌', 'Could not start Edge with CDP');
    return;
  }
  
  // Wait for page to load
  await sleep(3000);
  
  // Extract and push
  await extractFlow();
}

/**
 * Push SSO token to grok2api
 */
async function pushToGrok2Api(ssoToken) {
  log('🚀', 'Pushing token to grok2api...');
  
  try {
    // Check existing tokens
    const listResp = await httpReq('GET', `${CONFIG.GROK2API_URL}/admin/api/tokens`);
    
    if (listResp.status === 200 && listResp.data.tokens) {
      const existing = listResp.data.tokens;
      log('📊', `Current grok2api tokens: ${existing.length}`);
      
      // If token already exists, edit it; otherwise add new
      if (existing.length > 0) {
        // Replace the existing token with the new one
        const oldToken = existing[0].token;
        if (oldToken === ssoToken) {
          log('✅', 'Token unchanged, skipping update.');
          return;
        }
        
        // Edit (replace) existing token
        const editResp = await httpReq('PUT', `${CONFIG.GROK2API_URL}/admin/api/tokens/edit`, {
          old_token: oldToken,
          token: ssoToken,
          pool: existing[0].pool || 'auto',
        });
        
        if (editResp.status === 200) {
          log('✅', `Token REPLACED successfully! ${JSON.stringify(editResp.data)}`);
        } else {
          log('⚠️', `Edit failed (${editResp.status}), trying add instead...`);
          await addNewToken(ssoToken);
        }
      } else {
        await addNewToken(ssoToken);
      }
    } else {
      await addNewToken(ssoToken);
    }
    
  } catch (err) {
    log('❌', `grok2api error: ${err.message}`);
  }
}

async function addNewToken(ssoToken) {
  const addResp = await httpReq('POST', `${CONFIG.GROK2API_URL}/admin/api/tokens/add`, {
    tokens: [ssoToken],
    pool: 'auto',
    tags: ['auto-refresh', new Date().toISOString().split('T')[0]],
  });
  
  if (addResp.status === 200) {
    log('✅', `Token ADDED! ${JSON.stringify(addResp.data)}`);
  } else {
    log('❌', `Add failed: ${addResp.status} — ${JSON.stringify(addResp.data)}`);
  }
}

/**
 * Status check
 */
async function statusCheck() {
  log('📊', 'Checking grok2api status...');
  
  try {
    const resp = await httpReq('GET', `${CONFIG.GROK2API_URL}/admin/api/tokens`);
    
    if (resp.status === 200 && resp.data.tokens) {
      const tokens = resp.data.tokens;
      console.log(`\n${'═'.repeat(60)}`);
      console.log(`  grok2api — ${tokens.length} token(s)`);
      console.log(`${'═'.repeat(60)}\n`);
      
      for (const t of tokens) {
        const masked = t.token.length > 20 
          ? `${t.token.substring(0, 8)}...${t.token.substring(t.token.length - 8)}` 
          : t.token;
        console.log(`  Token:  ${masked}`);
        console.log(`  Pool:   ${t.pool} | Status: ${t.status} | Uses: ${t.use_count}`);
        if (t.quota) {
          const quotaStr = Object.entries(t.quota)
            .map(([k, v]) => `${k}: ${v.remaining}/${v.total}`)
            .join(' | ');
          console.log(`  Quota:  ${quotaStr}`);
        }
        console.log('');
      }
    }
    
    // Test a quick chat
    log('🧪', 'Testing Grok API...');
    const testResp = await httpReq('GET', `${CONFIG.GROK2API_URL}/v1/models`);
    if (testResp.status === 200) {
      const models = (testResp.data.data || []).map(m => m.id).join(', ');
      log('✅', `Models: ${models}`);
    }
    
  } catch (err) {
    log('❌', `Status error: ${err.message}`);
  }
}

// ═══════════════════════════════════════════
// CLI
// ═══════════════════════════════════════════

async function main() {
  const mode = process.argv[2] || '--status';
  
  console.log('\n🧠 Grok Cookie Refresh v2 (CDP-based)');
  console.log(`   grok2api: ${CONFIG.GROK2API_URL}`);
  console.log(`   CDP port: ${CONFIG.CDP_PORT}\n`);
  
  switch (mode) {
    case '--login':  case 'login':    await loginFlow(); break;
    case '--extract': case 'extract':  await extractFlow(); break;
    case '--auto':   case 'auto':     await autoFlow(); break;
    case '--status': case 'status':   await statusCheck(); break;
    default:
      console.log('Commands:');
      console.log('  --login    Open Edge browser, login to grok.com manually');
      console.log('  --extract  Extract cookie from running Edge → push to grok2api');
      console.log('  --auto     Full auto: launch Edge headless → extract → push');
      console.log('  --status   Check current grok2api token status');
  }
}

main().catch(err => {
  log('💥', `Fatal: ${err.message}`);
  process.exit(1);
});
