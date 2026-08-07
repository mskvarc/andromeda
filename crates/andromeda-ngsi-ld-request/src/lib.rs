//! What an NGSI-LD client asked for, as the specification spells it.
//!
//! ETSI GS CIM 009 v1.9.1 defines a request two ways: as query parameters on a
//! `GET` (clause 6.3), and as a JSON body on the `POST` equivalents (clauses
//! 5.7.2, 6.4.3.2, 6.34.3.2). They are the same request, and the body form is
//! defined by conversion into the parameter form, so both live here together
//! with the conversion between them.
//!
//! Nothing here is a web framework's type. A parameter set is deserialised from
//! a query string with `serde_urlencoded` and a body with `serde`; extracting
//! either from a live request is the binding layer's job. That is what lets the
//! forwarding client build a request for another broker out of the same types
//! the server reads one with.
//!
//! `deny_unknown_fields` is on every parameter struct deliberately: clause
//! 6.3.1 has a broker reject a request it does not fully understand, and a
//! silently ignored `?limitt=1000` is a wrong answer rather than a slow one.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_deletion;
pub mod body;
pub mod case_insensitive_bool;
pub mod csource_subscription;
pub mod deferred_split_filters;
pub mod discovery;
pub mod empty;
pub mod entity;
pub mod entity_operations;
pub mod error;
pub mod jsonld_context;
pub mod local;
pub mod protocol;
pub mod query;
pub mod representation_params;
pub mod subscription;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// A request is expressible in terms of the lexical, query and
    /// representation layers, plus the typed HTTP header values a protocol
    /// extension carries (clause 6.3.x), so nothing above those -- a resource, a
    /// store, a web framework -- may be named here.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-http-headers",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-representation",
        ];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a specification crate
    /// can spell an entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
