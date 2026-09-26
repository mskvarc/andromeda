-- ============================================================================
-- 0017: Schema contract stamp
-- ============================================================================
-- With a database per tenant, the schema exists in as many copies as there
-- are tenants, and any copy can drift: a migration that failed part-way
-- through the fleet, a tenant restored from an older backup, manual surgery.
-- The broker checks this stamp when it first connects to a tenant, so drift
-- is refused there instead of surfacing in the first failing query.
--
-- The row is not written here: a migration cannot compute the digest of the
-- set it belongs to. The broker writes the one row after the migrations have
-- run and verifies it on every later connection, so an empty table means
-- "migrated, never stamped", which the check reports as drift. The digest
-- covers the tenant schema's migrations, the live branch relations included,
-- and not the platform migrations the default tenant's database also carries.
-- ============================================================================

CREATE TABLE schema_metadata (
    applied_at TIMESTAMPTZ NOT NULL,
    schema_version INTEGER NOT NULL,
    singleton BOOLEAN NOT NULL DEFAULT TRUE,
    migrations_sha256 TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_schema_metadata PRIMARY KEY (singleton),
    CONSTRAINT chk_schema_metadata_singleton CHECK (singleton),
    CONSTRAINT chk_schema_metadata_version_positive CHECK (schema_version > 0),
    CONSTRAINT chk_schema_metadata_digest_hex CHECK (migrations_sha256 ~ '^[0-9a-f]{64}$')
);

REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON schema_metadata FROM PUBLIC;
GRANT SELECT ON schema_metadata TO PUBLIC;
