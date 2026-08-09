//! How an NGSI-LD resource is written onto the wire.
//!
//! ETSI GS CIM 009 v1.9.1 defines what a client receives in two halves: what
//! the resource is, which is clause 5.2, and how it appears, which is clauses
//! 4.5.1 (the normalized rendering), 4.5.4 (`keyValues` and simplified), the
//! per-type `.3` subclauses (concise), 4.15 (language negotiation), 4.8 (whether
//! the system-generated attributes appear), 4.5.23 (`join=inline`), 4.3.6.8
//! (version conformance) and 4.5.16 (the GeoJSON envelope). This crate is the second
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

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod concise;
pub mod context;
pub mod entity;
pub mod error;
pub mod geojson;
pub mod inline_join;
pub mod language_negotiation;
pub mod registration;
pub mod response;
pub mod simplify;
pub mod snapshot;
pub mod subscription;
pub mod temporal;
pub mod uri_decoding;
pub mod version;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-headers",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
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
