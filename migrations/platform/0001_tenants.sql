-- ============================================================================
-- 0001: Tenant registry (platform database)
-- ============================================================================
-- Maps every tenant to the database that holds its state. This lives in the
-- platform database, which is also the unnamed default tenant's database, and
-- is recorded in the platform ledger `_andromeda_platform_migrations`, apart
-- from the tenant schema's own ledger.
--
-- The unnamed default tenant is the row with `is_unnamed_default = TRUE` and
-- `name IS NULL`, never a row named "default": clause 6.3.14 gives the default
-- tenant no name, and a name would make it addressable by one.
-- `chk_tenants_unnamed_default_has_no_name` pins both halves -- a named row is
-- never the default and the default row is never named -- and
-- `idx_tenants_unnamed_default` admits at most one default row.
--
-- `name` is `COLLATE "C"`: a tenant name is an identifier compared for
-- equality through its unique index, never ordered linguistically.
--
-- `status` is a code of the migration-owned registry the broker compiles in
-- (`andromeda-postgresql-codes` `TenantStatusCode`):
--
--   1 active, 2 suspended
--
-- The registry is append-only and never renumbered. The check pins the range:
-- a number outside it could only come from a release that knows a status this
-- schema does not.
-- ============================================================================

CREATE TABLE tenants (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status SMALLINT NOT NULL,
    is_unnamed_default BOOLEAN NOT NULL DEFAULT FALSE,
    name TEXT COLLATE "C" UNIQUE,
    db_name TEXT COLLATE "C" NOT NULL UNIQUE,
    CONSTRAINT chk_tenants_unnamed_default_has_no_name CHECK ((name IS NULL) = is_unnamed_default),
    CONSTRAINT chk_tenants_status CHECK (status BETWEEN 1 AND 2)
);

CREATE UNIQUE INDEX idx_tenants_unnamed_default
    ON tenants (is_unnamed_default)
    WHERE is_unnamed_default = TRUE;
