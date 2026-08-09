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
pub mod uri_decoding;
pub mod version;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// A rendering reads a resource, the representation options that select
    /// one, and the header values that carry those options. It must never
    /// reach a query, a store, a context resolver or a web framework: the
    /// moment it does, deciding what to answer and spelling the answer are one
    /// thing again.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-layer-guard",
            "andromeda-http-headers",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-vocabulary",
        ];

        andromeda_layer_guard::manifest::assert_layer(
            env!("CARGO_MANIFEST_DIR"),
            andromeda_layer_guard::manifest::BASELINE_FORBIDDEN,
            ALLOWED_IN_WORKSPACE,
        );
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
