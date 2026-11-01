-- ============================================================================
-- 0003: Forwarded subscription routes
-- ============================================================================
-- Spec: ETSI GS CIM 009 v1.9.1 clauses 5.8.1.4, 6.3.14
--
-- Every subscription identifier this broker mints and hands to a peer, mapped
-- to the tenant that owns it: the companion registration subscription of a
-- local subscription, and each reduced copy placed at a context source
-- (clause 5.8.1.4). An inbound notification names only the minted identifier
-- and, at most, the peer's own tenant (clause 6.3.14), so this table is how
-- the receiving broker finds the tenant database to continue in. The unnamed
-- default tenant has routes like any other.
-- ============================================================================
create table forwarded_subscription_routes (
    tenant_key bigint not null,
    minted_subscription_id text compression lz4 collate "C" not null,
    constraint pk_forwarded_subscription_routes primary key (minted_subscription_id),
    constraint fk_forwarded_subscription_routes_tenant foreign key (tenant_key) references tenants (tenant_key) on delete cascade,
    constraint chk_forwarded_subscription_routes_minted_id_length check (octet_length(minted_subscription_id) <= 1024)
);

create index idx_forwarded_subscription_routes_tenant on forwarded_subscription_routes (tenant_key);
