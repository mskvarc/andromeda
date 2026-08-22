//! MAP area — EntityMap, subscription-to-CSR-subscription forwarding,
//! notification re-assembly, Context Source Identity, and the EntityMap /
//! ContextSourceIdentity data-shape constraints (ETSI GS CIM 009 v1.9.1,
//! clauses 5.8.1/5.8.2/5.8.5/5.8.6, 5.14.1-5.14.5, 5.15.1, 5.2.39, 5.2.40).
//!
//! Registry-API-local subscription and EntityMap validation runs on `T-LOCAL`
//! (broker A alone). Reduced-copy subscription forwarding and forwarded
//! EntityMap-query building run on `T-MOCK` against an in-process Context Source
//! that captures the exact forwarded request. Inbound notification re-assembly
//! runs on `T-SPLIT` (A -> B -> C, B and C each holding part of the same
//! entity). Concrete designs are in `docs/distributed-tests/map.md`.

pub mod csr_subscription;
mod data_shapes;
mod entity_map_crud;
mod entity_map_query;
mod entity_map_query_forwarding;
mod entity_map_request;
mod entity_map_temporal;
mod entity_map_temporal_forwarding;
mod entity_map_temporal_stages;
mod notification_body;
mod notification_content;
mod notification_delivery;
mod notification_format;
mod notification_inspection;
mod notification_reassembly;
mod source_identity;
mod subscription_create;
mod subscription_document;
mod subscription_forwarding;
mod subscription_forwarding_update;
mod subscription_notification;
mod subscription_update;
mod temporal_vehicle;
