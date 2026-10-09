-- ============================================================================
-- 0018: Schema contract stamp
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
create table schema_metadata (
    applied_at timestamptz not null,
    schema_version integer not null,
    singleton boolean not null default true,
    migrations_sha256 text compression lz4 collate "C" not null,
    constraint pk_schema_metadata primary key (singleton),
    constraint chk_schema_metadata_singleton check (singleton),
    constraint chk_schema_metadata_version_positive check (schema_version > 0),
    constraint chk_schema_metadata_digest_hex check (migrations_sha256 ~ '^[0-9a-f]{64}$')
);

revoke insert, update, delete, truncate on schema_metadata from public;

grant select on schema_metadata to public;

