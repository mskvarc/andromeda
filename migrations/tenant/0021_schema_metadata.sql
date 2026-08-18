-- ============================================================================
-- 0021: Per-tenant schema contract singleton
-- ============================================================================
-- Database-per-tenant means the schema exists in as many copies as there are
-- tenants, and every copy can drift: a migration that fails mid-fleet, a tenant
-- restored from an older backup, manual surgery. This table is what the broker
-- checks when it first connects to a tenant, so drift is refused there instead
-- of being discovered by the first failing query.
--
-- The row is deliberately not written here. A migration cannot compute the
-- digest of the migration set it belongs to, so the broker upserts the single
-- row after the embedded migrator finishes and verifies it on every later
-- connection. An empty table therefore means "migrated, never stamped", which
-- the startup check reports as drift rather than tolerating.
--
-- Privileges: this deployment has no runtime-only database role -- the broker
-- connects as the database owner, and an owner keeps every right on its own
-- tables whatever is revoked. The pair below is the durable half of the intent:
-- any role that is not the owner reads and cannot write, so introducing a
-- separate runtime role later needs no further migration.
--
-- Column order is physical, as every relation in this schema now is: not-null
-- fixed-width columns first in descending alignment, variable-length last.
-- ============================================================================

CREATE TABLE schema_metadata (
    applied_at TIMESTAMPTZ NOT NULL,
    schema_version INTEGER NOT NULL,
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    migrations_sha256 TEXT COLLATE "C" NOT NULL,
    CONSTRAINT chk_schema_metadata_singleton CHECK (singleton),
    CONSTRAINT chk_schema_metadata_version_positive CHECK (schema_version > 0),
    CONSTRAINT chk_schema_metadata_digest_hex CHECK (migrations_sha256 ~ '^[0-9a-f]{64}$')
);

REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON schema_metadata FROM PUBLIC;
GRANT SELECT ON schema_metadata TO PUBLIC;
