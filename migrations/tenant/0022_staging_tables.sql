-- ============================================================================
-- 0022: Staging-table helpers for the COPY-binary bulk-write path
-- ============================================================================
-- The actual staging tables are created on demand per transaction via
-- `CREATE TEMP TABLE ... ON COMMIT DROP` in the Rust repository layer; they
-- are session-scoped rather than migration-defined so the shape can evolve
-- without a schema migration. This file is an intentional marker for the
-- ordering of the feature in the migration history.
-- ============================================================================

SELECT 1;
