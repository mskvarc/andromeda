-- ============================================================================
-- 0018: Partial index for notification_outbox cleanup (B.6)
-- ============================================================================
-- `OutboxRepository::delete_delivered_before` scans for entries with
-- `delivered_at IS NOT NULL AND delivered_at < $1`. Without a partial index,
-- the cleanup runs a full sequential scan of the outbox, whose row count is
-- dominated by already-delivered rows until cleanup catches up.
--
-- A partial btree on `delivered_at` restricted to `delivered_at IS NOT NULL`
-- lets the cleanup DELETE plan an index range scan while avoiding any index
-- maintenance cost during the far more common "insert pending entry" path.
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_outbox_delivered
    ON notification_outbox (delivered_at)
    WHERE delivered_at IS NOT NULL;
