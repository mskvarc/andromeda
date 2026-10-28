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
--   status: 1 active, 2 suspended, 3 provisional
--
-- A provisional tenant is registered for a creating operation that targets it
-- for the first time (clause 5.5.10). Its row is committed before its database
-- is created, so an interrupted provisioning leaves a row and never a database
-- the registry does not name. It is answered as nonexistent until its database
-- holds information, when it becomes active. One that stays empty is retired
-- once its `modified_at`, which every provisioning refreshes, is older than the
-- configured time to live: its database is dropped, then its row deleted.
--
-- The unnamed default tenant is the row with `is_unnamed_default` set and no
-- `name`, never a row named "default": clause 6.3.14 gives the default tenant
-- no name, and a name would make it addressable by one. The unique partial
-- index admits at most one such row.
-- ============================================================================
create table tenants (
    tenant_key bigint generated always as identity,
    created_at timestamptz not null default now(),
    modified_at timestamptz not null default now(),
    status smallint not null,
    is_unnamed_default boolean not null default false,
    name text compression lz4 collate "C",
    database_name text compression lz4 collate "C" not null,
    constraint pk_tenants primary key (tenant_key),
    constraint uq_tenants_name unique (name),
    constraint uq_tenants_database_name unique (database_name),
    constraint chk_tenants_unnamed_default_has_no_name check ((name is null) = is_unnamed_default),
    constraint chk_tenants_status check (status between 1 and 3),
    constraint chk_tenants_name_length check (octet_length(name) <= 1005),
    constraint chk_tenants_database_name_length check (octet_length(database_name) <= 63)
);

create unique index uq_tenants_unnamed_default on tenants (is_unnamed_default)
where
    is_unnamed_default = true;
