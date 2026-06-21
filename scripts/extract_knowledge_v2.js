/**
 * Knowledge Extractor v2 — Local extraction (no LLM needed)
 * 
 * Pipeline:
 * 1. Parse all conversation logs
 * 2. Extract knowledge locally using patterns/heuristics
 * 3. Build searchable knowledge base
 * 4. Generate HTML viewer
 * 
 * Usage: node extract_knowledge_v2.js
 */

import { readFileSync, writeFileSync, readdirSync, existsSync, statSync, mkdirSync } from 'fs';
import { join } from 'path';

const BRAIN_DIR = 'C:\\Users\\thang\\.gemini\\antigravity\\brain';
const OUTPUT_DIR = 'E:\\AGT_Brain\\data\\knowledge';

// ─── Parse all conversations ───
function parseConversations() {
  const conversations = [];
  const dirs = readdirSync(BRAIN_DIR);
  
  for (const dir of dirs) {
    const overviewPath = join(BRAIN_DIR, dir, '.system_generated', 'logs', 'overview.txt');
    if (!existsSync(overviewPath)) continue;
    
    const stat = statSync(overviewPath);
    if (stat.size < 100) continue;
    
    const content = readFileSync(overviewPath, 'utf-8');
    const lines = content.split('\n').filter(l => l.trim());
    
    const steps = [];
    for (const line of lines) {
      try { steps.push(JSON.parse(line)); } catch {}
    }
    
    if (steps.length < 3) continue;
    
    conversations.push({ id: dir, steps });
  }
  
  return conversations;
}

// ─── Extract knowledge from a single conversation ───
function extractKnowledge(conv) {
  const { id, steps } = conv;
  const knowledge = {
    conversation_id: id,
    date: steps[0]?.created_at || 'unknown',
    last_date: steps[steps.length - 1]?.created_at || 'unknown',
    total_steps: steps.length,
    user_requests: [],
    files_modified: new Set(),
    files_created: new Set(),
    files_viewed: new Set(),
    commands_run: [],
    tools_used: {},
    decisions: [],
    errors_and_fixes: [],
    key_topics: [],
    code_patterns: [],
    configs: [],
  };

  for (const step of steps) {
    // ─ User requests ─
    if (step.source === 'USER_EXPLICIT' && step.content) {
      const match = step.content.match(/<USER_REQUEST>\n?([\s\S]*?)\n?<\/USER_REQUEST>/);
      if (match) {
        knowledge.user_requests.push(match[1].trim());
      }
    }

    // ─ Model content analysis ─
    if (step.source === 'MODEL' && step.content) {
      const content = step.content;
      
      // Detect decisions (usually contain "→", "nên", "decided", "chose")
      if (content.match(/(?:→|decided|chose|nên dùng|quyết định|recommendation|con sẽ|approach)/i)) {
        if (content.length > 30 && content.length < 1000) {
          knowledge.decisions.push(content.substring(0, 500));
        }
      }
      
      // Detect errors/fixes
      if (content.match(/(?:❌|error|lỗi|fix|sửa|bug|failed|crash)/i)) {
        if (content.length > 20 && content.length < 800) {
          knowledge.errors_and_fixes.push(content.substring(0, 400));
        }
      }
    }

    // ─ Tool calls analysis ─
    if (step.tool_calls) {
      for (const tc of step.tool_calls) {
        // Count tool usage
        knowledge.tools_used[tc.name] = (knowledge.tools_used[tc.name] || 0) + 1;
        
        const args = tc.args || {};
        
        // File modifications
        if (tc.name === 'replace_file_content' || tc.name === 'multi_replace_file_content') {
          const file = args.TargetFile;
          if (file) knowledge.files_modified.add(file);
          if (args.Description) knowledge.code_patterns.push({
            file: file,
            action: 'modified',
            description: args.Description,
          });
        }
        
        // File creations
        if (tc.name === 'write_to_file') {
          const file = args.TargetFile;
          if (file) knowledge.files_created.add(file);
          if (args.Description) knowledge.code_patterns.push({
            file: file,
            action: 'created',
            description: args.Description,
          });
        }
        
        // Files viewed
        if (tc.name === 'view_file') {
          const file = args.AbsolutePath;
          if (file) knowledge.files_viewed.add(file);
        }
        
        // Commands run
        if (tc.name === 'run_command') {
          const cmd = args.CommandLine;
          if (cmd && cmd.length < 500) {
            knowledge.commands_run.push({
              command: cmd,
              cwd: args.Cwd || '',
            });
          }
        }
        
        // Config changes
        if (tc.name === 'mcp_github_push_files' || tc.name === 'mcp_github_create_or_update_file') {
          knowledge.configs.push({
            tool: tc.name,
            path: args.path,
            message: args.message,
          });
        }
      }
    }
  }

  // Convert Sets to Arrays
  knowledge.files_modified = [...knowledge.files_modified];
  knowledge.files_created = [...knowledge.files_created];
  knowledge.files_viewed = [...knowledge.files_viewed];

  // Infer topic from user requests
  const allRequests = knowledge.user_requests.join(' ').toLowerCase();
  const topics = [];
  if (allRequests.match(/supabase|memory|database/)) topics.push('Memory/Database');
  if (allRequests.match(/grok|subagent|gravity/)) topics.push('Grok Integration');
  if (allRequests.match(/mcp|tool|server/)) topics.push('MCP Server');
  if (allRequests.match(/cdp|browser|automat/)) topics.push('CDP Automation');
  if (allRequests.match(/deploy|docker|vps|contabo/)) topics.push('Deployment');
  if (allRequests.match(/chatgpt|openai/)) topics.push('ChatGPT Integration');
  if (allRequests.match(/dashboard|html|web/)) topics.push('Web Dashboard');
  if (allRequests.match(/rust|cargo|crate/)) topics.push('Rust Development');
  if (allRequests.match(/git|github|commit/)) topics.push('Git/GitHub');
  if (allRequests.match(/debug|fix|error|bug/)) topics.push('Debugging');
  if (allRequests.match(/config|setting|env/)) topics.push('Configuration');
  if (allRequests.match(/register|google|account/)) topics.push('Account Registration');
  if (allRequests.match(/efashion|shop|login/)) topics.push('EFashionShop');
  if (allRequests.match(/cursor|vscode|ide/)) topics.push('IDE/Editor');
  if (allRequests.match(/rule|agents\.md|gemini\.md/)) topics.push('Rules/Config');
  knowledge.key_topics = topics.length > 0 ? topics : ['General'];

  return knowledge;
}

// ─── Generate summary stats ───
function generateStats(knowledgeBase) {
  const stats = {
    total_conversations: knowledgeBase.length,
    total_steps: 0,
    total_user_requests: 0,
    total_files_modified: new Set(),
    total_files_created: new Set(),
    total_commands: 0,
    total_decisions: 0,
    total_errors: 0,
    tool_frequency: {},
    topic_frequency: {},
    date_range: { from: '9999', to: '0000' },
  };

  for (const kb of knowledgeBase) {
    stats.total_steps += kb.total_steps;
    stats.total_user_requests += kb.user_requests.length;
    kb.files_modified.forEach(f => stats.total_files_modified.add(f));
    kb.files_created.forEach(f => stats.total_files_created.add(f));
    stats.total_commands += kb.commands_run.length;
    stats.total_decisions += kb.decisions.length;
    stats.total_errors += kb.errors_and_fixes.length;
    
    for (const [tool, count] of Object.entries(kb.tools_used)) {
      stats.tool_frequency[tool] = (stats.tool_frequency[tool] || 0) + count;
    }
    
    kb.key_topics.forEach(t => {
      stats.topic_frequency[t] = (stats.topic_frequency[t] || 0) + 1;
    });
    
    if (kb.date < stats.date_range.from) stats.date_range.from = kb.date;
    if (kb.last_date > stats.date_range.to) stats.date_range.to = kb.last_date;
  }

  stats.total_files_modified = stats.total_files_modified.size;
  stats.total_files_created = stats.total_files_created.size;
  
  // Sort tools by frequency
  stats.tool_frequency = Object.fromEntries(
    Object.entries(stats.tool_frequency).sort((a, b) => b[1] - a[1])
  );

  return stats;
}

// ─── Generate HTML Viewer ───
function generateHTMLViewer(knowledgeBase, stats) {
  const sortedKB = [...knowledgeBase].sort((a, b) => b.date.localeCompare(a.date));
  
  return `<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>🧠 Antigravity Knowledge Base</title>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body { 
      font-family: 'Segoe UI', system-ui, sans-serif; 
      background: #0a0a0f; color: #e0e0e0;
      line-height: 1.6;
    }
    .container { max-width: 1400px; margin: 0 auto; padding: 20px; }
    
    /* Header */
    .header {
      background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
      border-radius: 16px; padding: 32px; margin-bottom: 24px;
      border: 1px solid #2a2a4a;
    }
    .header h1 { 
      font-size: 2rem; color: #fff;
      background: linear-gradient(90deg, #667eea, #764ba2);
      -webkit-background-clip: text; -webkit-text-fill-color: transparent;
    }
    .header .subtitle { color: #8888aa; margin-top: 4px; }
    
    /* Stats Grid */
    .stats-grid {
      display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
      gap: 12px; margin-top: 20px;
    }
    .stat-card {
      background: rgba(255,255,255,0.05); border-radius: 10px;
      padding: 16px; text-align: center;
      border: 1px solid rgba(255,255,255,0.08);
    }
    .stat-card .value { font-size: 1.8rem; font-weight: bold; color: #667eea; }
    .stat-card .label { font-size: 0.75rem; color: #888; text-transform: uppercase; }
    
    /* Search */
    .search-bar {
      width: 100%; padding: 14px 20px; font-size: 1rem;
      background: #12121a; color: #fff; border: 1px solid #2a2a4a;
      border-radius: 12px; margin-bottom: 20px; outline: none;
    }
    .search-bar:focus { border-color: #667eea; }
    
    /* Filters */
    .filters { display: flex; gap: 8px; flex-wrap: wrap; margin-bottom: 20px; }
    .filter-btn {
      padding: 6px 14px; border-radius: 20px; border: 1px solid #2a2a4a;
      background: transparent; color: #888; cursor: pointer; font-size: 0.8rem;
    }
    .filter-btn.active { background: #667eea; color: #fff; border-color: #667eea; }
    
    /* Conversation Cards */
    .conv-card {
      background: #12121a; border-radius: 12px; padding: 20px;
      margin-bottom: 16px; border: 1px solid #1e1e2e;
      transition: border-color 0.2s;
    }
    .conv-card:hover { border-color: #667eea; }
    .conv-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; }
    .conv-date { color: #667eea; font-size: 0.85rem; }
    .conv-id { color: #555; font-size: 0.75rem; font-family: monospace; }
    .conv-topics { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 12px; }
    .topic-tag {
      padding: 2px 10px; border-radius: 12px; font-size: 0.7rem;
      background: rgba(102, 126, 234, 0.15); color: #667eea;
    }
    
    /* Sections */
    .section { margin-top: 12px; }
    .section-title { 
      font-size: 0.8rem; color: #888; text-transform: uppercase;
      letter-spacing: 1px; margin-bottom: 6px; cursor: pointer;
    }
    .section-title:hover { color: #667eea; }
    .section-content { display: none; }
    .section-content.open { display: block; }
    
    .item {
      padding: 8px 12px; margin: 4px 0; border-radius: 8px;
      background: rgba(255,255,255,0.03); font-size: 0.85rem;
      border-left: 3px solid transparent;
    }
    .item.request { border-left-color: #4caf50; }
    .item.decision { border-left-color: #ff9800; }
    .item.error { border-left-color: #f44336; }
    .item.file-mod { border-left-color: #2196f3; }
    .item.file-new { border-left-color: #00bcd4; }
    .item.command { border-left-color: #9c27b0; font-family: monospace; font-size: 0.8rem; }
    
    .code { font-family: 'Cascadia Code', monospace; font-size: 0.8rem; color: #e8a0bf; }
    
    /* Tool chart */
    .tool-bar { display: flex; align-items: center; gap: 8px; margin: 4px 0; }
    .tool-name { width: 180px; font-size: 0.8rem; color: #aaa; text-align: right; }
    .tool-fill { height: 20px; background: linear-gradient(90deg, #667eea, #764ba2); border-radius: 4px; min-width: 4px; }
    .tool-count { font-size: 0.75rem; color: #667eea; }
    
    .hidden { display: none; }
  </style>
</head>
<body>
  <div class="container">
    <div class="header">
      <h1>🧠 Antigravity Knowledge Base</h1>
      <div class="subtitle">Extracted from ${stats.total_conversations} conversations • ${stats.date_range.from.substring(0, 10)} → ${stats.date_range.to.substring(0, 10)}</div>
      
      <div class="stats-grid">
        <div class="stat-card"><div class="value">${stats.total_conversations}</div><div class="label">Conversations</div></div>
        <div class="stat-card"><div class="value">${stats.total_steps}</div><div class="label">Total Steps</div></div>
        <div class="stat-card"><div class="value">${stats.total_user_requests}</div><div class="label">User Requests</div></div>
        <div class="stat-card"><div class="value">${stats.total_files_modified}</div><div class="label">Files Modified</div></div>
        <div class="stat-card"><div class="value">${stats.total_files_created}</div><div class="label">Files Created</div></div>
        <div class="stat-card"><div class="value">${stats.total_commands}</div><div class="label">Commands Run</div></div>
        <div class="stat-card"><div class="value">${stats.total_decisions}</div><div class="label">Decisions</div></div>
        <div class="stat-card"><div class="value">${stats.total_errors}</div><div class="label">Errors Fixed</div></div>
      </div>
    </div>
    
    <input type="text" class="search-bar" placeholder="🔍 Search knowledge base... (type to filter)" oninput="filterCards(this.value)">
    
    <div class="filters" id="topicFilters">
      <button class="filter-btn active" onclick="filterByTopic('all')">All</button>
      ${Object.entries(stats.topic_frequency)
        .sort((a, b) => b[1] - a[1])
        .map(([topic, count]) => `<button class="filter-btn" onclick="filterByTopic('${topic}')">${topic} (${count})</button>`)
        .join('\n      ')}
    </div>
    
    <div id="conversations">
      ${sortedKB.map(kb => `
      <div class="conv-card" data-topics="${kb.key_topics.join(',')}" data-search="${escapeHtml((kb.user_requests.join(' ') + ' ' + kb.files_modified.join(' ') + ' ' + kb.files_created.join(' ') + ' ' + kb.decisions.join(' ')).toLowerCase())}">
        <div class="conv-header">
          <div class="conv-date">📅 ${kb.date.substring(0, 10)} ${kb.date.substring(11, 16)}</div>
          <div class="conv-id">${kb.conversation_id.substring(0, 8)} • ${kb.total_steps} steps</div>
        </div>
        <div class="conv-topics">
          ${kb.key_topics.map(t => `<span class="topic-tag">${t}</span>`).join('')}
        </div>
        
        ${kb.user_requests.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">💬 User Requests (${kb.user_requests.length}) ▸</div>
          <div class="section-content">
            ${kb.user_requests.map(r => `<div class="item request">${escapeHtml(r.substring(0, 300))}</div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.files_created.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">📝 Files Created (${kb.files_created.length}) ▸</div>
          <div class="section-content">
            ${kb.files_created.map(f => `<div class="item file-new"><span class="code">${escapeHtml(f)}</span></div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.files_modified.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">✏️ Files Modified (${kb.files_modified.length}) ▸</div>
          <div class="section-content">
            ${kb.files_modified.map(f => `<div class="item file-mod"><span class="code">${escapeHtml(f)}</span></div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.code_patterns.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">🔧 Code Changes (${kb.code_patterns.length}) ▸</div>
          <div class="section-content">
            ${kb.code_patterns.slice(0, 20).map(p => `<div class="item decision"><strong>${p.action}:</strong> <span class="code">${escapeHtml(p.file || '')}</span><br>${escapeHtml(p.description || '')}</div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.decisions.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">🎯 Decisions (${kb.decisions.length}) ▸</div>
          <div class="section-content">
            ${kb.decisions.slice(0, 10).map(d => `<div class="item decision">${escapeHtml(d.substring(0, 300))}</div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.errors_and_fixes.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">🐛 Errors & Fixes (${kb.errors_and_fixes.length}) ▸</div>
          <div class="section-content">
            ${kb.errors_and_fixes.slice(0, 10).map(e => `<div class="item error">${escapeHtml(e.substring(0, 300))}</div>`).join('')}
          </div>
        </div>` : ''}
        
        ${kb.commands_run.length > 0 ? `
        <div class="section">
          <div class="section-title" onclick="toggleSection(this)">⚡ Commands (${kb.commands_run.length}) ▸</div>
          <div class="section-content">
            ${kb.commands_run.slice(0, 15).map(c => `<div class="item command">${escapeHtml(c.command.substring(0, 200))}</div>`).join('')}
          </div>
        </div>` : ''}
      </div>
      `).join('')}
    </div>
    
    <!-- Tool Usage Chart -->
    <div class="header" style="margin-top: 24px;">
      <h2 style="color: #667eea; margin-bottom: 16px;">🔧 Tool Usage Frequency</h2>
      ${Object.entries(stats.tool_frequency).slice(0, 20).map(([tool, count]) => {
        const maxCount = Object.values(stats.tool_frequency)[0];
        const width = Math.max(4, (count / maxCount) * 400);
        return `<div class="tool-bar">
          <div class="tool-name">${tool}</div>
          <div class="tool-fill" style="width: ${width}px;"></div>
          <div class="tool-count">${count}</div>
        </div>`;
      }).join('\n      ')}
    </div>
  </div>
  
  <script>
    function toggleSection(el) {
      const content = el.nextElementSibling;
      content.classList.toggle('open');
      el.textContent = el.textContent.replace(/[▸▾]/, content.classList.contains('open') ? '▾' : '▸');
    }
    
    function filterCards(query) {
      const q = query.toLowerCase();
      document.querySelectorAll('.conv-card').forEach(card => {
        const searchText = card.dataset.search || '';
        card.classList.toggle('hidden', q.length > 0 && !searchText.includes(q));
      });
    }
    
    function filterByTopic(topic) {
      document.querySelectorAll('.filter-btn').forEach(btn => btn.classList.remove('active'));
      event.target.classList.add('active');
      
      document.querySelectorAll('.conv-card').forEach(card => {
        if (topic === 'all') {
          card.classList.remove('hidden');
        } else {
          const topics = card.dataset.topics || '';
          card.classList.toggle('hidden', !topics.includes(topic));
        }
      });
    }
  </script>
</body>
</html>`;
}

function escapeHtml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// ─── Main ───
function main() {
  console.log('🧠 Knowledge Extractor v2 — Local extraction\n');
  
  console.log('📖 Parsing conversation logs...');
  const conversations = parseConversations();
  console.log(`   Found ${conversations.length} conversations\n`);
  
  try { mkdirSync(OUTPUT_DIR, { recursive: true }); } catch {}
  
  const knowledgeBase = [];
  
  for (const conv of conversations) {
    const shortId = conv.id.substring(0, 8);
    console.log(`🔍 Processing ${shortId}...`);
    
    const knowledge = extractKnowledge(conv);
    knowledgeBase.push(knowledge);
    
    console.log(`   ✅ ${knowledge.key_topics.join(', ')} | ${knowledge.user_requests.length} requests, ${knowledge.files_modified.length} files modified, ${knowledge.commands_run.length} commands`);
  }
  
  // Generate stats
  const stats = generateStats(knowledgeBase);
  
  // Save JSON
  const kbPath = join(OUTPUT_DIR, 'knowledge_base.json');
  writeFileSync(kbPath, JSON.stringify(knowledgeBase, null, 2), 'utf-8');
  console.log(`\n💾 Knowledge base → ${kbPath}`);
  
  // Save stats
  const statsPath = join(OUTPUT_DIR, 'stats.json');
  writeFileSync(statsPath, JSON.stringify(stats, null, 2), 'utf-8');
  
  // Generate HTML viewer
  const htmlPath = join(OUTPUT_DIR, 'knowledge_viewer.html');
  writeFileSync(htmlPath, generateHTMLViewer(knowledgeBase, stats), 'utf-8');
  console.log(`🌐 HTML Viewer → ${htmlPath}`);
  
  // Summary
  console.log(`\n📊 Summary:`);
  console.log(`   Conversations: ${stats.total_conversations}`);
  console.log(`   Total steps: ${stats.total_steps}`);
  console.log(`   User requests: ${stats.total_user_requests}`);
  console.log(`   Files modified: ${stats.total_files_modified}`);
  console.log(`   Files created: ${stats.total_files_created}`);
  console.log(`   Commands run: ${stats.total_commands}`);
  console.log(`   Decisions captured: ${stats.total_decisions}`);
  console.log(`   Errors documented: ${stats.total_errors}`);
  console.log(`\n🎉 Done! Open knowledge_viewer.html in browser.`);
}

main();
