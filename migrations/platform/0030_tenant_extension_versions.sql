-- ============================================================================
-- 0030: Tenant extension versions
-- ============================================================================
-- The TimescaleDB, PostGIS and pg_repack versions each tenant database held
-- after the broker last opened it, and the version the server shipped at that
-- moment. A database keeps the extension version it was created or last
-- updated at until a broker opens it, and the installed version lives only in
-- each database's own catalogue, so this table is how the platform database
-- finds the tenants still behind an image upgrade without connecting to any of
-- them. The unnamed default tenant has rows like any other.
--
-- Every open writes the rows of the tenant it opened, after the extensions
-- were brought up to date, so the open path is the table's one writer.
-- `installed_version` is NULL when the extension is not installed in the
-- tenant database, and `shipped_version` when the server ships no version of
-- it. A tenant opened while the server shipped the version it ships now has
-- had its one attempt against that version, whatever the attempt left
-- installed.
--
-- The migrations create TimescaleDB and PostGIS, and an open updates them.
-- pg_repack is optional: an open installs it when the server ships it and the
-- connecting role may create it, and drops and creates it again when it is
-- installed below the shipped version, since it ships no update scripts. Its
-- row with a NULL `installed_version` beside a `shipped_version` is a tenant
-- whose open could not install it.
-- ============================================================================
create table tenant_extension_versions (
    tenant_key bigint not null,
    reconciled_at timestamptz not null,
    extension text compression lz4 collate "C" not null,
    installed_version text compression lz4 collate "C",
    shipped_version text compression lz4 collate "C",
    constraint pk_tenant_extension_versions primary key (tenant_key, extension),
    constraint fk_tenant_extension_versions_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_tenant_extension_versions_extension check (extension in ('postgis', 'timescaledb', 'pg_repack')),
    constraint chk_tenant_extension_versions_extension_length check (octet_length(extension) <= 63),
    constraint chk_tenant_extension_versions_installed_version_length check (octet_length(installed_version) <= 64),
    constraint chk_tenant_extension_versions_shipped_version_length check (octet_length(shipped_version) <= 64)
);
