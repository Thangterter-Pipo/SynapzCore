/**
 * Knowledge Extractor — Trích xuất kiến thức từ conversation logs
 * 
 * Pipeline:
 * 1. Parse tất cả conversation logs (JSONL)
 * 2. Tóm tắt từng conversation thành digest
 * 3. Gửi digest qua Grok Heavy để trích xuất kiến thức
 * 4. Lưu thành knowledge_base.json
 * 
 * Usage: node extract_knowledge.js
 */

import { readFileSync, writeFileSync, readdirSync, existsSync, statSync } from 'fs';
import { join } from 'path';

const BRAIN_DIR = 'C:\\Users\\thang\\.gemini\\antigravity\\brain';
const OUTPUT_DIR = 'E:\\AGT_Brain\\data\\knowledge';
const GROK_API = 'http://localhost:8000/v1/chat/completions';
const GROK_MODEL = 'grok-3';

// ─── Step 1: Parse all conversations ───
function parseConversations() {
  const conversations = [];
  const dirs = readdirSync(BRAIN_DIR);
  
  for (const dir of dirs) {
    const overviewPath = join(BRAIN_DIR, dir, '.system_generated', 'logs', 'overview.txt');
    if (!existsSync(overviewPath)) continue;
    
    const stat = statSync(overviewPath);
    if (stat.size < 100) continue; // Skip empty/tiny
    
    const content = readFileSync(overviewPath, 'utf-8');
    const lines = content.split('\n').filter(l => l.trim());
    
    const steps = [];
    for (const line of lines) {
      try {
        const step = JSON.parse(line);
        steps.push(step);
      } catch (e) {
        // Skip malformed lines
      }
    }
    
    if (steps.length < 3) continue;
    
    // Extract user messages and model responses (content only)
    const userMessages = steps
      .filter(s => s.source === 'USER_EXPLICIT' && s.content)
      .map(s => {
        // Extract just the USER_REQUEST part
        const match = s.content?.match(/<USER_REQUEST>\n?([\s\S]*?)\n?<\/USER_REQUEST>/);
        return match ? match[1].trim() : s.content?.substring(0, 500);
      })
      .filter(Boolean);
    
    const modelSummaries = steps
      .filter(s => s.source === 'MODEL' && s.content && s.content.length > 20)
      .map(s => s.content.substring(0, 800))
      .slice(0, 15); // Limit to keep digest manageable
    
    // Extract tool calls for context
    const toolsUsed = new Set();
    steps.forEach(s => {
      if (s.tool_calls) {
        s.tool_calls.forEach(tc => toolsUsed.add(tc.name));
      }
    });
    
    const firstDate = steps[0]?.created_at || 'unknown';
    const lastDate = steps[steps.length - 1]?.created_at || 'unknown';
    
    conversations.push({
      id: dir,
      date: firstDate,
      lastDate,
      userMessages,
      modelSummaries,
      toolsUsed: [...toolsUsed],
      totalSteps: steps.length,
      userMsgCount: userMessages.length,
    });
  }
  
  return conversations.sort((a, b) => a.date.localeCompare(b.date));
}

// ─── Step 2: Create digest for Grok ───
function createDigest(conv) {
  let digest = `## Conversation ${conv.id.substring(0, 8)}\n`;
  digest += `Date: ${conv.date} → ${conv.lastDate}\n`;
  digest += `Steps: ${conv.totalSteps} | User msgs: ${conv.userMsgCount}\n`;
  digest += `Tools: ${conv.toolsUsed.join(', ')}\n\n`;
  
  digest += `### User Requests:\n`;
  conv.userMessages.forEach((msg, i) => {
    digest += `${i + 1}. ${msg.substring(0, 300)}\n`;
  });
  
  digest += `\n### Key Model Responses:\n`;
  conv.modelSummaries.forEach((msg, i) => {
    digest += `${i + 1}. ${msg.substring(0, 500)}\n`;
  });
  
  // Truncate to keep fast
  return digest.substring(0, 3500);
}

// ─── Step 3: Call Grok to extract knowledge ───
async function extractWithGrok(digest, convId) {
  const systemPrompt = `You are a Knowledge Extraction Engine. Analyze the conversation digest and extract ALL valuable knowledge into structured categories.

Output ONLY valid JSON (no markdown, no code fences) with this exact structure:
{
  "topic": "Main topic of conversation in 1 line",
  "decisions": [{"what": "...", "why": "...", "context": "..."}],
  "patterns": [{"name": "...", "description": "...", "when_to_use": "..."}],
  "bugs_and_fixes": [{"bug": "...", "cause": "...", "fix": "..."}],
  "architecture": [{"component": "...", "design": "...", "rationale": "..."}],
  "preferences": [{"preference": "...", "detail": "..."}],
  "tools_learned": [{"tool": "...", "usage": "...", "tip": "..."}],
  "key_insights": ["insight1", "insight2"]
}

Rules:
- Extract EVERYTHING useful, don't skip
- If a category has no items, use empty array
- Be specific with code/config details
- Include file paths, commands, config values when mentioned
- Focus on reusable knowledge, not conversation mechanics`;

  const body = JSON.stringify({
    model: GROK_MODEL,
    messages: [
      { role: 'system', content: systemPrompt },
      { role: 'user', content: `Extract knowledge from this conversation:\n\n${digest}` }
    ],
    max_tokens: 4000,
    temperature: 0.1,
  });

  try {
    const response = await fetch(GROK_API, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
      signal: AbortSignal.timeout(120000), // 2 min timeout
    });

    if (!response.ok) {
      const err = await response.text();
      console.error(`  ❌ API error for ${convId.substring(0, 8)}: ${err}`);
      return null;
    }

    // Handle SSE streaming response (grok2api always streams)
    const rawText = await response.text();
    let content = '';
    
    // Check if it's SSE format
    if (rawText.startsWith('data: ')) {
      const lines = rawText.split('\n');
      for (const line of lines) {
        if (line.startsWith('data: ') && line !== 'data: [DONE]') {
          try {
            const chunk = JSON.parse(line.slice(6));
            const delta = chunk.choices?.[0]?.delta?.content || '';
            content += delta;
          } catch {}
        }
      }
    } else {
      // Regular JSON response
      try {
        const data = JSON.parse(rawText);
        content = data.choices?.[0]?.message?.content || '';
      } catch {
        content = rawText;
      }
    }
    
    if (!content) return null;
    
    // Try to parse JSON response
    try {
      const cleaned = content.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim();
      return JSON.parse(cleaned);
    } catch (e) {
      console.error(`  ⚠️ JSON parse failed for ${convId.substring(0, 8)}, saving raw`);
      return { raw: content, parse_error: true };
    }
  } catch (e) {
    console.error(`  ❌ Fetch error for ${convId.substring(0, 8)}: ${e.message}`);
    return null;
  }
}

// ─── Step 4: Main pipeline ───
async function main() {
  console.log('🧠 Knowledge Extractor — Starting...\n');
  
  // Parse conversations
  console.log('📖 Step 1: Parsing conversation logs...');
  const conversations = parseConversations();
  console.log(`   Found ${conversations.length} conversations with logs\n`);
  
  // Create output directory
  const { mkdirSync } = await import('fs');
  try { mkdirSync(OUTPUT_DIR, { recursive: true }); } catch {}
  
  // Process each conversation
  const knowledgeBase = [];
  let processed = 0;
  let failed = 0;
  
  for (const conv of conversations) {
    processed++;
    const shortId = conv.id.substring(0, 8);
    console.log(`🔍 [${processed}/${conversations.length}] Processing ${shortId} (${conv.date.substring(0, 10)}, ${conv.totalSteps} steps)...`);
    
    const digest = createDigest(conv);
    const knowledge = await extractWithGrok(digest, conv.id);
    
    if (knowledge) {
      knowledgeBase.push({
        conversation_id: conv.id,
        date: conv.date,
        steps: conv.totalSteps,
        tools_used: conv.toolsUsed,
        ...knowledge,
      });
      console.log(`   ✅ Extracted: ${knowledge.topic || 'untitled'}`);
    } else {
      failed++;
      console.log(`   ❌ Failed to extract`);
    }
    
    // Small delay to avoid rate limiting
    await new Promise(r => setTimeout(r, 2000));
  }
  
  // Save knowledge base
  const outputPath = join(OUTPUT_DIR, 'knowledge_base.json');
  writeFileSync(outputPath, JSON.stringify(knowledgeBase, null, 2), 'utf-8');
  console.log(`\n💾 Saved knowledge base to ${outputPath}`);
  
  // Also create a flat searchable index
  const searchIndex = [];
  for (const kb of knowledgeBase) {
    const addItems = (category, items) => {
      if (!Array.isArray(items)) return;
      items.forEach(item => {
        searchIndex.push({
          conversation_id: kb.conversation_id,
          date: kb.date,
          topic: kb.topic,
          category,
          content: typeof item === 'string' ? item : JSON.stringify(item),
        });
      });
    };
    
    addItems('decision', kb.decisions);
    addItems('pattern', kb.patterns);
    addItems('bug_fix', kb.bugs_and_fixes);
    addItems('architecture', kb.architecture);
    addItems('preference', kb.preferences);
    addItems('tool', kb.tools_learned);
    addItems('insight', kb.key_insights);
  }
  
  const indexPath = join(OUTPUT_DIR, 'search_index.json');
  writeFileSync(indexPath, JSON.stringify(searchIndex, null, 2), 'utf-8');
  
  // Summary
  console.log(`\n📊 Summary:`);
  console.log(`   Conversations processed: ${processed}`);
  console.log(`   Successful extractions: ${processed - failed}`);
  console.log(`   Failed: ${failed}`);
  console.log(`   Total knowledge items: ${searchIndex.length}`);
  console.log(`   Output: ${outputPath}`);
  console.log(`   Search index: ${indexPath}`);
}

main().catch(console.error);
