//! How an NGSI-LD resource is written onto the wire.
//!
//! ETSI GS CIM 009 v1.9.1 defines what a client receives in two halves: what
//! the resource is, which is clause 5.2, and how it appears, which is clauses
//! 4.5.1 (the normalized rendering), 4.5.4 (`keyValues` and simplified), the
//! per-type `.3` subclauses (concise), 4.15 (language negotiation), 4.8 (whether
//! the system-generated attributes appear), 4.5.23 (`join=inline`), 4.3.6.8
//! (version conformance) and 4.5.16 (the `GeoJSON` envelope). This crate is the second
//! half, and only the second half: nothing here decides *what* to answer, only
//! how the answer is spelled.
//!
//! Every renderer is a consuming builder — inputs through the constructor and
//! `with_*` methods, then `render`. There is no `Renderer` trait: the thirteen
//! renderings share a name and nothing else, and a trait over them would only
//! force one error type onto types that fail for unrelated reasons, or for no
//! reason at all. The five renderings that cannot fail return their output
//! directly rather than a `Result` nothing ever inspects.
//!
//! What a client *asks* for is not here. Media types, `Prefer` tokens and
//! language ranges arrive as HTTP headers and are spelled in
//! `andromeda-http-headers`; this crate reads the values they carry and
//! never the request they came from.

pub mod body_response;
mod concise;
pub mod context;
pub mod contextual_body;
mod entity_body;
pub mod entity_map_body;
pub mod entity_response;
#[cfg(test)]
mod entity_response_fallback_tests;
#[cfg(test)]
mod entity_response_tests;
pub mod error;
pub mod forwarded_entity;
pub mod forwarded_fragment;
pub mod forwarded_merge_entity;
pub mod forwarded_subscription;
pub mod geometry_selection;
pub mod joined_notification;
pub mod language_negotiation;
#[cfg(test)]
mod list_as_relationship_tests;
pub mod notification_data;
pub mod notification_entity;
pub mod notification_language;
pub mod notified_changes;
#[cfg(test)]
mod notified_changes_tests;
pub mod registration;
pub mod response;
pub mod structural_projection;
pub mod subscription;
pub mod temporal;
pub mod version;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-headers",
        "andromeda-json-literal",
        "andromeda-linked-retrieval",
        "andromeda-native-stack",
        "andromeda-ngsi-ld-entity",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
