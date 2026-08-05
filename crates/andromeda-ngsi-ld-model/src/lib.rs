//! The Andromeda NGSI-LD domain model.
//!
//! This crate holds the resources ETSI GS CIM 009 v1.9.1 defines — entities,
//! attributes, subscriptions, registrations and the discovery resources — built
//! on the lexical types of `andromeda-ngsi-ld-lexicon` and the IRI constants of
//! `andromeda-ngsi-ld-vocabulary`. It knows nothing about transport,
//! persistence, or the broker runtime, and every layer above it depends on
//! these types rather than on each other's.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute;
pub mod attribute_list;
pub mod entity;
pub mod entity_map;
pub mod entity_type;
pub mod entity_type_list;
pub mod expanded;
pub mod jsonld_context;
pub mod notification;
pub mod outbox;
pub mod registration;
pub mod response_format;
pub mod snapshot;
pub mod snapshot_filter;
pub mod snapshot_query;
pub mod subscription;
pub mod sys_attrs;
pub mod tenant_list;
