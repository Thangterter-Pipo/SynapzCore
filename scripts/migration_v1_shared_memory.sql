-- Shared Memory Schema Migration v1
-- Adds agent, session_id, category, importance, confidence columns
-- Date: 2026-04-17

-- New columns
ALTER TABLE memories ADD COLUMN IF NOT EXISTS agent TEXT DEFAULT 'antigravity';
ALTER TABLE memories ADD COLUMN IF NOT EXISTS session_id TEXT;
ALTER TABLE memories ADD COLUMN IF NOT EXISTS category TEXT DEFAULT 'general';
ALTER TABLE memories ADD COLUMN IF NOT EXISTS importance SMALLINT DEFAULT 3;
ALTER TABLE memories ADD COLUMN IF NOT EXISTS confidence SMALLINT DEFAULT 3;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_memories_agent ON memories (agent);
CREATE INDEX IF NOT EXISTS idx_memories_category ON memories (category);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories (importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_agent_cat ON memories (agent, category);

-- Backfill existing records
UPDATE memories SET agent = 'antigravity' WHERE agent IS NULL;
UPDATE memories SET category = 'general' WHERE category IS NULL;
UPDATE memories SET importance = 3 WHERE importance IS NULL;
UPDATE memories SET confidence = 3 WHERE confidence IS NULL;
