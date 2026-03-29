//! NGSI-LD type system with full serialization/deserialization support
//! for all three NGSI-LD representations (normalized, concise, simplified).
//!
//! This crate provides the core NGSI-LD types as defined in ETSI GS CIM 009:
//!
//! - [`Entity`] and [`EntityPatch`] — entity representations
//! - [`Attribute`] — the six attribute types (Property, Relationship, GeoProperty, etc.)
//! - [`Uri`] — NGSI-LD entity identifiers (URN or URL)
//! - [`NameBuf`] — validated attribute/type names
//! - [`ScopeBuf`] and [`NgsiLdScope`] — entity scopes
//! - [`NgsiLdContext`] — JSON-LD `@context` values
//! - [`batch`] — batch operation result types
//! - [`representation`] — serialization for normalized/concise/simplified formats
//! - [`iri`] — NGSI-LD expanded IRI constants

pub mod attribute;
pub mod attribute_type;
pub mod context;
pub mod entity;
pub mod error;
pub mod expanded;
pub mod iri;
pub mod name;
pub mod notification;
pub mod registration;
pub mod representation;
pub mod response_format;
pub mod scope;
pub mod subscription;
pub mod temporal;
#[cfg(test)]
mod tests;
pub mod time_format;
pub mod uri;

pub use attribute::{Attribute, AttributeWrapper, Attributes};
pub use attribute_type::AttributeType;
pub use context::NgsiLdContext;
pub use entity::{Entity, EntityPatch, EntityType, Scope};
pub use error::{Error, Result};
pub use name::NameBuf;
pub use registration::{CsrOperation, ForwardingMode};
pub use representation::{NgsiLdRepresentation, NgsiLdSerializable, NgsiLdSkipNull, ToRepresentation};
pub use response_format::{JoinMode, ResponseFormat};
pub use scope::{NgsiLdScope, ScopeBuf};
pub use subscription::NotificationTrigger;
pub use temporal::ValueType;
pub use uri::Uri;
