//! The NGSI-LD Query Language (ETSI GS CIM 009 v1.9.1 clause 4.9).
//!
//! The `q` query parameter selects entities by the values of their attributes.
//! This module owns the language: the grammar it parses into, the
//! recursive-descent parser, term expansion through the active JSON-LD context,
//! and serialisation back to clause 4.9 syntax. How a filter is evaluated, in
//! memory or in a storage dialect, is a property of that execution and lives
//! with it.
//!
//! A filter exists in two name stages, one type over both: a `Filter<Written>`
//! holds the terms as the client wrote them and is what the parser yields, and a
//! `Filter<Expanded>` holds every attribute name and entity type as the absolute
//! IRI the request `@context` expands it to, which is what every execution
//! reads. A tree crosses to the expanded stage through `expand_filter` (the
//! `@context` expands the terms), or through `Filter::into_expanded` when a
//! stored text already spells absolute IRIs and only needs checking. It crosses
//! back through `compact_filter`, which spells the IRIs in the terms of a
//! reader's `@context`, or `Filter::into_written`, which keeps the IRIs as
//! text. The sub-attributes and the members of a trailing group are
//! [`AttributeName`](andromeda_ngsi_ld_lexicon::attribute_name::AttributeName)s
//! in both stages, and `MemberPath::written` keeps the spelling the client wrote
//! beside the names the members address: a `LanguageProperty` target language or
//! a raw JSON key is read as written (clause 4.9), and no `@context` rewrites it.

pub mod attribute_path;
pub mod compaction;
pub mod compilation;
pub mod compiled_filter;
pub mod complexity;
pub mod display;
pub mod equality_value;
pub mod expansion;
pub mod expectation;
pub mod filter_error;
#[cfg(test)]
pub(crate) mod generated_filters;
pub mod hop_admission;
pub mod instance_metadata;
pub mod into_expanded;
pub mod into_written;
pub mod json_key_attributes;
pub mod language_matching;
pub mod language_range;
pub mod language_targeting;
pub mod nested_names;
pub mod operand_coercion;
pub mod operator_spelling;
mod parser;
pub mod query_term;
pub mod quoting;
pub mod reading;
pub mod referenced_attributes;
#[cfg(test)]
mod round_trip_tests;
pub mod scalar;
pub mod target_language;
pub mod trailing_group;
pub mod unicode_escape_error;
pub mod value_path;
pub mod written_members;

use crate::{
    filter::{filter_error::FilterError, query_term::QueryTerm, target_language::LanguageSelector, value_path::ValuePath},
    name_stage::{NameStage, Written},
};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// A parsed `q` filter: `And` and `Or` nodes combine sub-filters, `Term` leaves hold individual Query Terms.
///
/// The wire form is the clause 4.9 query text itself, in both directions: a
/// resource that carries a filter, such as a Subscription (clause 5.2.12), a
/// `ContextSourceRegistration` (5.2.9) or a Snapshot query (5.2.23), stores the
/// text a client wrote and must give back an equivalent one. Parsing at
/// deserialization is what makes a malformed filter a rejected request instead
/// of a filter that silently matches nothing at first use.
///
/// The tree is generic over the [`NameStage`] of its names: a `Filter<Written>`
/// is what the parser reads, and a `Filter<Expanded>` is what expansion through
/// the request `@context` makes and what an execution reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "String",
    into = "String",
    bound(
        serialize = "S: NameStage",
        deserialize = "S: NameStage, Filter<S>: TryFrom<String>, <Filter<S> as TryFrom<String>>::Error: Display"
    )
)]
pub enum Filter<S: NameStage> {
    /// Conjunction: all sub-filters must be true (`;` in NGSI-LD syntax).
    And(Vec<Filter<S>>),
    /// Disjunction: at least one sub-filter must be true (`|` in NGSI-LD syntax).
    Or(Vec<Filter<S>>),
    /// A leaf query term.
    Term(QueryTerm<S>),
}

impl<S: NameStage> Filter<S> {
    /// Whether any query term in this filter traverses a linked entity (the
    /// clause 4.9 `{}` axis).
    ///
    /// A consumer evaluating a filter in memory needs to know: a traversal
    /// reaches an attribute of another entity, which has to be read before the
    /// filter can be decided.
    pub fn contains_linked_hop(&self) -> bool {
        match self {
            Filter::And(filters) | Filter::Or(filters) => filters.iter().any(Filter::contains_linked_hop),
            Filter::Term(term) => term.path().is_linked(),
        }
    }

    /// The greatest number of Relationship hops any query term in this filter
    /// crosses (the clause 4.9 `{}` axis, counted).
    ///
    /// Clause 5.7.2.4 bounds a filter's traversal by the request's `joinLevel`,
    /// and the bound applies to the deepest hop chain the filter contains: a
    /// shallow term standing beside a deep one must not hide the deep one.
    /// Zero when the filter stays inside the entity under evaluation.
    pub fn linked_depth(&self) -> u32 {
        match self {
            Filter::And(filters) | Filter::Or(filters) => filters.iter().map(Filter::linked_depth).max().unwrap_or(0),
            Filter::Term(term) => term.path().linked_depth(),
        }
    }

    /// Every Query Term, in pre-order.
    #[must_use]
    pub fn terms(&self) -> Vec<&QueryTerm<S>> {
        match self {
            Filter::And(filters) | Filter::Or(filters) => filters.iter().flat_map(Filter::terms).collect(),
            Filter::Term(term) => vec![term],
        }
    }

    /// The value path every Query Term terminates at, in pre-order.
    #[must_use]
    pub fn terminal_paths(&self) -> Vec<&ValuePath<S>> {
        match self {
            Filter::And(filters) | Filter::Or(filters) => filters.iter().flat_map(Filter::terminal_paths).collect(),
            Filter::Term(term) => vec![term.path().terminal()],
        }
    }

    /// The value path every Query Term terminates at, mutably, in pre-order.
    pub fn terminal_paths_mut(&mut self) -> Vec<&mut ValuePath<S>> {
        match self {
            Filter::And(filters) | Filter::Or(filters) => filters.iter_mut().flat_map(Filter::terminal_paths_mut).collect(),
            Filter::Term(term) => vec![term.path_mut().terminal_mut()],
        }
    }

    /// Every target language the filter's Query Terms name, in pre-order.
    ///
    /// A one-member group spelled like a language tag (`color[en]`) counts:
    /// whether it reads a language or a compound member is decided by the
    /// attribute it meets, which the filter alone cannot tell.
    #[must_use]
    pub fn target_languages(&self) -> Vec<LanguageSelector<'_>> {
        self.terminal_paths().into_iter().filter_map(ValuePath::target_language).collect()
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for Filter<Written> {
    type Error = FilterError;

    fn try_from(raw: String) -> Result<Filter<Written>, FilterError> {
        raw.parse()
    }
}

// The wire form is produced by `Display`; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl<S: NameStage> From<Filter<S>> for String {
    fn from(filter: Filter<S>) -> String {
        filter.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{filter::Filter, name_stage::Written};

    #[test]
    fn a_filter_round_trips_through_its_clause_4_9_text() {
        // Clause 4.9: the wire form of a stored filter is its query text, so a
        // resource that is read back must yield a filter that parses to the
        // same tree.
        let raw = "speed>50;brandName==\"Mercedes\"";
        let json = serde_json::to_string(&raw.parse::<Filter<Written>>().expect("valid filter")).expect("serialises");
        pretty_assertions::assert_eq!(json, format!("{raw:?}"));
        let decoded: Filter<Written> = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, raw.parse::<Filter<Written>>().expect("valid filter"));
    }

    /// Clause 5.7.2.4 measures a filter against `joinLevel` by hop count, so a
    /// local filter must measure zero, one brace group one, a chain two, and a
    /// disjunction of unequal depths the deeper of the two.
    #[test]
    fn the_linked_depth_is_the_deepest_hop_chain_the_filter_contains() {
        let cases = [
            ("speed>50", 0),
            ("isParkedIn{category}==\"commercial\"", 1),
            ("isParkedIn{inDistrict{zone}}==\"north\"", 2),
            ("speed>50;isParkedIn{category}", 1),
            ("speed>50|isParkedIn{inDistrict{zone}}", 2),
        ];

        for (raw, expected) in cases {
            let filter = raw.parse::<Filter<Written>>().expect("valid filter");
            pretty_assertions::assert_eq!(filter.linked_depth(), expected, "depth of {raw}");
        }
    }

    #[test]
    fn a_malformed_filter_is_rejected_when_the_resource_is_deserialized() {
        let error = serde_json::from_str::<Filter<Written>>("\"speed>\"").expect_err("must reject");
        assert!(error.to_string().contains("q expression"), "got {error}");
    }
}
