-- ============================================================================
-- 014: Dedup index for attribute_history
-- ============================================================================
-- The bulk dedup DELETE uses WHERE ah.tea_id = d.tea_id AND ah.instance_id =
-- d.instance_id. The existing UNIQUE constraint (tea_id, time, instance_id)
-- cannot serve this lookup because `time` sits between the two dedup columns.
-- This index covers the (tea_id, instance_id) axis directly.
-- ============================================================================

CREATE INDEX idx_attribute_history_dedup
    ON attribute_history(tea_id, instance_id);
