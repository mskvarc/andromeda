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
//! `andromeda-http-headers` beside those headers rather than being
//! split from them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod entity_members;
pub mod join_mode;
pub mod normalization;
pub mod response_format;
pub mod sys_attrs;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
