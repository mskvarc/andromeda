//! The Andromeda NGSI-LD domain model.
//!
//! This crate holds the types ETSI GS CIM 009 v1.9.1 defines — entities,
//! attributes, subscriptions, registrations and the discovery resources — along
//! with the value types they are built from. It is the leaf of the workspace:
//! it knows nothing about transport, persistence, or the broker runtime, and
//! every layer above it depends on these types rather than on each other's.

#![deny(missing_docs)]

pub mod attribute;
pub mod attribute_list;
pub mod attribute_path;
pub mod attribute_type;
pub mod constant_string;
pub mod entity;
pub mod entity_map;
pub mod entity_type;
pub mod entity_type_list;
pub mod expanded;
pub mod geo_rel;
pub mod geometry;
pub mod health;
pub mod jsonld_context;
pub mod name;
pub mod ngsi_ld_urn;
pub mod notification;
pub mod outbox;
pub mod registration;
pub mod response_format;
pub mod rfc3339;
pub mod scope;
pub mod snapshot;
pub mod snapshot_filter;
pub mod subscription;
pub mod temporal;
pub mod tenant;
pub mod uri;
pub mod version;
pub mod vocab;
