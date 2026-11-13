-- ============================================================================
-- 0036: Lockstep Snapshot branches
-- ============================================================================
-- Every Snapshot branch schema takes each branch migration before any later
-- migration runs in the tenant database, so every schema of the database is
-- at one version. No Snapshot records a version of its own, or a failure to
-- reach one: a branch that cannot take a migration keeps the tenant from
-- opening instead.
-- ============================================================================
alter table snapshot_preparation
    drop column branch_schema_version,
    drop column branch_upgrade_error;
