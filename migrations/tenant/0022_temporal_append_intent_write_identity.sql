-- ============================================================================
-- 0022: Align temporal append intents with the broker write identity
-- ============================================================================
-- Migration 0020 named this column `intent_id`, while the queue schema and all
-- runtime statements name the same durable identity `write_id`. Rename it in a
-- forward migration so databases that already recorded migration 0020 keep a
-- valid SQLx migration checksum. PostgreSQL updates the existing unique
-- constraint to reference the renamed column without rebuilding its data.
-- ============================================================================

ALTER TABLE temporal_append_intents RENAME COLUMN intent_id TO write_id;
