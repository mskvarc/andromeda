-- ============================================================================
-- 0037: Name the atomic write identity `write_id`, and persist its recorded_at
-- ============================================================================
-- 0030 introduced `intent_id`/`intent_ordinal` on `attribute_history` and an
-- `intent_id` receipt per accepted append. That identifier is not really the
-- durable queue row's identity: it is the identity of one atomic broker write,
-- and the same value is what will correlate history, the notification outbox and
-- deferred side effects. `write_id`/`write_ordinal` say that; `intent_id` said
-- only where the value happened to be minted.
--
-- The queue table keeps its name and its `intent` payload column: those genuinely
-- name the durable append intent. Only the identity columns are renamed, in
-- `attribute_history`, `temporal_append_intents` and
-- `temporal_history_sink_receipts`, together with the one index name that
-- spelled the old column on `attribute_history`. `temporal_append_intents`'s
-- own uniqueness constraint was already given an explicit name at creation, so
-- renaming its column does not require renaming it too.
--
-- Renaming a column on `attribute_history` is a catalogue-only change that
-- TimescaleDB propagates to every chunk, columnar chunks included, so no
-- rowstore conversion and no policy juggling is needed here.
--
-- `recorded_at` on the receipt exists because the deduplicating key of the
-- rebuilt hypertable is (recorded_at, write_id, write_ordinal), and every unique
-- index on a hypertable must contain the partition column. A replay that
-- re-stamped ingestion time would therefore never collide with the rows it
-- duplicates. Persisting the batch's `recorded_at` on the receipt makes a replay
-- reuse the original timestamp, so the collision happens. Existing receipts have
-- no recorded batch timestamp; `accepted_at` is the closest truth available and
-- is the only value that can be reconstructed, so it is what they get before the
-- column becomes NOT NULL.
--
-- This migration runs in one transaction: it is metadata-only plus one update of
-- a catalogue-scale table, and atomicity is what makes it re-runnable.
-- ============================================================================

ALTER TABLE attribute_history RENAME COLUMN intent_id TO write_id;
ALTER TABLE attribute_history RENAME COLUMN intent_ordinal TO write_ordinal;
ALTER INDEX idx_attribute_history_intent RENAME TO idx_attribute_history_write;

ALTER TABLE temporal_append_intents RENAME COLUMN intent_id TO write_id;

ALTER TABLE temporal_history_sink_receipts RENAME COLUMN intent_id TO write_id;

ALTER TABLE temporal_history_sink_receipts ADD COLUMN recorded_at TIMESTAMPTZ;

UPDATE temporal_history_sink_receipts
SET recorded_at = accepted_at
WHERE recorded_at IS NULL;

ALTER TABLE temporal_history_sink_receipts ALTER COLUMN recorded_at SET NOT NULL;
