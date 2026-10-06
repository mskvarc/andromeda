//! CONS area — consumption forwarding across Context Sources
//! (ETSI GS CIM 009 v1.9.1, clauses 5.7.1–5.7.11, merge 4.5.5, term expansion
//! 5.5.7, distributed pagination 5.5.9).
//!
//! Retrieve/Query Entity and Retrieve/Query Temporal forwarding, the four
//! registration modes, per-attribute merge and `datasetId` filtering, expired
//! instance discard, auxiliary supplementation, split-entity filter timing,
//! entity-map create/reuse, and system-wide available types/attributes.
//!
//! Topology mapping (plan §3): `T-LOCAL`/`T-MOCK`/`T-MOCK-N` → [`single`] plus an
//! in-process [`ContextSourceAdapter`]; `T-PAIR` → [`pair`]; `T-SPLIT`/`T-CHAIN`
//! → [`chain`], with brokers B and C each holding part of one entity. Entity
//! maps are durable and shared across instances, so the cross-instance
//! linked-map rows (`REQ-CONS-8/13/26/45/48/61`) run like any other row.
//!
//! [`single`]: `crate::topology::{self`, `BrokerRole}::single`
//! [`pair`]: `crate::topology::{self`, `BrokerRole}::pair`
//! [`chain`]: `crate::topology::{self`, `BrokerRole}::chain`
//! [`ContextSourceAdapter`]: `crate::context_source_adapter::ContextSourceAdapter`

mod aggregated_temporal_sources;
mod available_types_attrs;
mod entity_map_query;
mod entity_map_reuse;
mod entity_map_temporal;
mod federated_last_n_order;
mod forward_binding;
mod merge_temporal_values;
mod query_entities;
mod query_entities_filters;
mod query_temporal;
mod query_temporal_filters;
mod query_temporal_validation;
mod retrieve_entity;
mod retrieve_entity_projection;
mod retrieve_temporal;
mod retrieve_temporal_filters;
mod split_entities;
mod split_temporal;
mod split_temporal_settlement;
