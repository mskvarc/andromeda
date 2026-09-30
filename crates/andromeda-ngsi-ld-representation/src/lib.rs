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
//! resource type exists to hold it. [`attribute_type_validation`] is its
//! gatekeeper: a node that declares a `type` outside clause 5.2.4 is rejected
//! before normalization could mistake it for a value.
//!
//! The three member rules beside them say which members an attribute of a given
//! family may carry at all. [`family_member_admission`] holds the fourteen
//! "Prohibited" lists of clauses 4.5.2 through 4.5.24 as the one rule they
//! are, [`proof_seal_admission`] the `ngsildproof` exception of clause 4.5.2.2,
//! and [`output_only_member`] the `previous…` member clauses 5.2.5 through
//! 5.2.38 say a Context Producer may not supply. They are stated here rather
//! than beside a store or a service because each is a statement about a
//! representation and nothing else, and because every layer that admits a
//! written attribute has to agree on them.
//!
//! [`embedded_context_admission`] is the fourth of those representation rules
//! and the only one about where a member may stand rather than which members
//! there may be: clause 5.5.7 admits a user `@context` at the top level of an
//! NGSI-LD document and nowhere else.
//!
//! How a client *asks* for one is not here. Media-type negotiation, version
//! conformance and language ranges arrive as HTTP headers and are spelled by
//! the header grammars that carry them, so they live in
//! `andromeda-http-headers` beside those headers rather than being
//! split from them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_type_validation;
pub mod embedded_context_admission;
pub mod entity_members;
pub mod family_member_admission;
pub mod join_mode;
pub mod linked_resolution_failure;
pub mod merge_representation;
pub mod normalization;
pub mod output_only_member;
pub mod proof_seal_admission;
pub mod response_format;
pub mod sys_attrs;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
