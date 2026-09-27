//! HTTP area — the NGSI-LD HTTP binding for distributed operations
//! (ETSI GS CIM 009 v1.9.1, clauses 6.3.7–6.3.21, 6.4–6.13, 6.31–6.35).
//!
//! Every submodule owns one facet of the binding: the response-code matrix for
//! entity, attribute and batch resources; format/representation negotiation;
//! the `NGSILD-Warning` fault-injection codes (110/111/199/299); the `Via`
//! cascade chain; the `NGSILD-EntityMap` request/response headers and the
//! `entityMaps` resource lifecycle; the distributed query/purge URL parameters;
//! the single-source error mapping (508/504/404/502/207); the Registry API and
//! CSR-subscription resources plus source identity; csource notification
//! dispatch headers; the `contextSourceInfo`-derived request headers; and the
//! `NGSILD-Tenant` / count / `NGSILD-Results-Count` headers.
//!
//! Local-only rows run on `T-LOCAL` (broker A alone). Fault-injection and
//! forwarded-request rows run on `T-MOCK` (broker A plus an in-process mock
//! Context Source registered by the test). The loop row runs on `T-LOOP`
//! (broker A registered back at itself) and the `Via`-append row on `T-CHAIN`.
//! Assertions that were reconciled to the specification are recorded in
//! `docs/distributed-operations-spec-corrections.md`.

mod contextsourceinfo_headers;
mod csource_registration;
mod csource_subscription;
mod entity_map_crud;
mod entity_map_header;
mod error_mapping;
mod forwarding;
mod notification_headers;
mod params;
mod representation;
mod status_codes_attr_lifecycle;
mod status_codes_attrs;
mod status_codes_entities;
mod status_codes_mutations;
mod status_codes_replace_merge;
mod tenant_count;
mod via;
mod warning_headers;
