-- ============================================================================
-- 0001: Tenant registry
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clause 6.3.14
--
-- Every tenant and the database that holds its state. The registry lives in
-- the platform database, which is also the database of the unnamed default
-- tenant.
--
-- Codes:
--
--   status: 1 active, 2 suspended
--
-- The unnamed default tenant is the row with `is_unnamed_default` set and no
-- `name`, never a row named "default": clause 6.3.14 gives the default tenant
-- no name, and a name would make it addressable by one. The unique partial
-- index admits at most one such row.
-- ============================================================================

CREATE TABLE tenants (
    tenant_key BIGINT GENERATED ALWAYS AS IDENTITY,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    modified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status SMALLINT NOT NULL,
    is_unnamed_default BOOLEAN NOT NULL DEFAULT FALSE,
    name TEXT COMPRESSION lz4 COLLATE "C",
    database_name TEXT COMPRESSION lz4 COLLATE "C" NOT NULL,
    CONSTRAINT pk_tenants PRIMARY KEY (tenant_key),
    CONSTRAINT uq_tenants_name UNIQUE (name),
    CONSTRAINT uq_tenants_database_name UNIQUE (database_name),
    CONSTRAINT chk_tenants_unnamed_default_has_no_name CHECK ((name IS NULL) = is_unnamed_default),
    CONSTRAINT chk_tenants_status CHECK (status BETWEEN 1 AND 2)
);

CREATE UNIQUE INDEX uq_tenants_unnamed_default
    ON tenants (is_unnamed_default)
    WHERE is_unnamed_default = TRUE;
