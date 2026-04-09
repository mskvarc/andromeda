//! Renderer layer: encapsulates all data transformations applied to domain
//! objects before they are sent as HTTP responses.
//!
//! Renderers sit between the service/domain layer and the HTTP response layer.
//! They handle:
//!
//! - **Entity rendering** ([`entity::EntityRenderer`]) -- language negotiation,
//!   inline join expansion, concise/simplified/normalized representation modes,
//!   snapshot rendering, and percent-encoded URI decoding.
//! - **GeoJSON mapping** ([`geojson::GeoJsonRenderer`]) -- wrapping entity JSON
//!   in GeoJSON Feature / FeatureCollection envelopes.
//! - **Registration rendering** ([`registration`]) -- stripping system attributes
//!   and runtime status from CSR payloads.
//! - **Response pipeline** ([`response::ResponseRenderer`]) -- central rendering
//!   pipeline applying URI decoding, version adaptation, `@context` injection,
//!   and GeoJSON wrapping.
//! - **Subscription rendering** ([`subscription`]) -- stripping system attributes
//!   and runtime notification fields from subscription payloads.
//! - **Temporal formatting** ([`temporal`]) -- building temporal entity
//!   representations from database rows, expanding GeoJSON-LD coordinates,
//!   aggregating temporal values, and building Content-Range headers.
//! - **Version adaptation** ([`version::VersionRenderer`]) -- adapting payloads
//!   to conform to older NGSI-LD spec versions when the client sends
//!   `Prefer: ngsi-ld=<version>`.
//!
//! API handlers configure renderers via builder methods and call `render()`.
//! No transformation logic should live in handlers directly.

/// Whether system-generated attributes (`createdAt`, `modifiedAt`) should be
/// included in the response or stripped before serialization.
///
/// Replaces bare `bool` parameters in subscription, registration, and temporal
/// rendering functions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SysAttrsInclusion {
    /// Include `createdAt`, `modifiedAt`, and related system attributes.
    Include,
    /// Strip system attributes from the response.
    #[default]
    Strip,
}

/// Where the JSON-LD `@context` should appear in the response.
///
/// For `application/json` responses the context goes in the `Link` header;
/// for `application/ld+json` it is embedded in the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLocation {
    /// Context is provided via the HTTP `Link` header.
    LinkHeader,
    /// Context is embedded in the JSON response body.
    Body,
}

/// Whether NGSI-LD version adaptation was applied to the response payload.
///
/// When a client sends `Prefer: ngsi-ld=<version>` and the broker adapts the
/// payload, the response uses 203 Non-Authoritative Information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAdaptation {
    /// The payload was adapted to an older NGSI-LD version.
    Applied,
    /// No version adaptation was performed.
    None,
}

pub mod entity;
pub mod geojson;
pub mod registration;
pub mod response;
pub mod subscription;
pub mod temporal;
pub mod version;
