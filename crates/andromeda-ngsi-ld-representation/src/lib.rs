//! How an NGSI-LD resource is rendered, as opposed to what it is.
//!
//! ETSI GS CIM 009 v1.9.1 separates a resource from its representation: clause
//! 4.5.1 names the normalized rendering, 4.5.4 the `keyValues`/simplified one
//! and the per-type `.3` subclauses the concise one; clause 4.8 says when the
//! system-generated attributes appear in one, and clause 4.5.23 says whether a
//! linked entity is nested or flat.
//! None of those decisions changes what an entity *is*, which is why they are
//! not members of the data model, and every one of them is read on both sides
//! of it — by the query languages that compile a request and by the renderers
//! that answer one.
//!
//! It sits below both: a representation option is expressible in terms of the
//! lexical layer alone, so nothing here names a resource, a query or a store.
//!
//! [`normalization`] is the one transformation that belongs here rather than
//! with a renderer: clause 4.5.2.3 lets a *request* abbreviate an attribute, and
//! rewriting that into the normalized form of clause 4.5.1 happens before any
//! resource type exists to hold it.
//!
//! How a client *asks* for one is not here. Media-type negotiation, version
//! conformance and language ranges arrive as HTTP headers and are spelled by
//! the header grammars that carry them, so they live in
//! `andromeda-ngsi-ld-http-headers` beside those headers rather than being
//! split from them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod join_mode;
pub mod normalization;
pub mod response_format;
pub mod sys_attrs;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    /// A representation option is expressible in the lexical layer alone, so
    /// nothing above it — a resource, a query, a store — may be named here.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "confique", "clap", "tokio"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-lexicon", "andromeda-ngsi-ld-vocabulary"];

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
