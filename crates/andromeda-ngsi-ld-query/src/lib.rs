//! The NGSI-LD query languages: their grammars, their ASTs and their compiled forms.
//!
//! ETSI GS CIM 009 v1.9.1 defines a family of query facilities, namely `q` (clause
//! 4.9), `georel` (4.10), `scopeQ` (4.18), type selection (4.17), `orderBy`
//! (4.23), projection (4.21) and the temporal window (4.5.19), that a broker
//! must read identically wherever they appear. This crate owns all of them: each
//! concept's grammar, its AST and its compiled model live in one directory, so
//! a change to one concept touches one directory rather than several pipeline
//! stages.
//!
//! Modules are therefore named after NGSI-LD concepts, never after stages. How
//! a compiled query is rendered as SQL is deliberately absent: that is a
//! property of a backend. Nothing here names a database.
//!
//! Evaluating a query against an entity held in memory is absent for the same
//! reason: it is an execution of the languages, not part of them.
//!
//! Term expansion is likewise inverted rather than depended upon: compilation
//! that needs a term expanded against an `@context` asks a
//! [`term_expander::TermExpander`], and a stored filter read back in a
//! reader's terms asks a [`term_compactor::TermCompactor`]; the JSON-LD engine
//! implements both.

pub mod bracket_expression;
pub mod candidate_filters;
pub mod comma_separated_list;
pub mod count_policy;
pub mod csr;
pub mod cursor;
pub mod dataset;
pub mod defined_token;
pub mod entity;
pub mod entity_filters;
pub mod entity_selection;
pub mod error;
pub mod filter;
pub mod geo;
pub mod identifier_axis;
#[cfg(any(test, feature = "test-support"))]
pub mod namespace_expander;
pub mod non_posix_construct;
pub mod ordering;
pub mod page_reach;
pub mod page_window;
#[cfg(test)]
mod parse_behaviour_tests;
pub mod pattern;
pub mod portable_pattern;
pub mod posix_dialect;
pub mod projection;
pub mod query_identity;
pub mod read_order;
pub mod scope;
pub mod selector_compilation;
pub mod split_filter;
pub mod standing_instant;
pub mod subject_start;
pub mod temporal;
pub mod term_compactor;
pub mod term_expander;
pub mod type_selection;
pub mod written_entity_selector;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-json-literal",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-value-order",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
