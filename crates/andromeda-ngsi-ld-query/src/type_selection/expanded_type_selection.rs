//! An entity type selection (clause 4.17) with every type name expanded to its IRI.

use crate::{query_error::QueryError, type_selection::written_type_selection::WrittenTypeSelection};
use iri_rs::IriBuf;
use serde::Serialize;
use std::slice;

/// Boolean expression over entity type IRIs (NGSI-LD S4.17).
///
/// Supports AND (`And`), OR (`Or`), and single type (`Type`) nodes.
/// Constructed by parsing the `type` query parameter's selection language
/// grammar and expanding compact type names to full IRIs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpandedTypeSelection {
    /// The clause-4.17 wildcard, selecting every expanded entity type.
    Wildcard,
    /// Matches entities whose types include this specific IRI.
    Type(IriBuf),
    /// Matches entities whose types satisfy all children (semicolon-separated).
    And(Vec<ExpandedTypeSelection>),
    /// Matches entities whose types satisfy any child (comma/pipe-separated).
    Or(Vec<ExpandedTypeSelection>),
}

impl ExpandedTypeSelection {
    /// Whether a set of expanded entity type IRIs satisfies this expression.
    ///
    /// The counterpart of [`WrittenTypeSelection::matches`] on the other side of
    /// expansion: the same clause 4.17 boolean structure, with leaves compared
    /// as IRIs rather than as compact names. A caller that has the expanded
    /// payload (a store that declined the type predicate, a notification
    /// matcher working on expanded JSON-LD) has IRIs and nothing to compact
    /// them against.
    pub fn matches<S: AsRef<str>>(&self, type_iris: &[S]) -> bool {
        match self {
            ExpandedTypeSelection::Wildcard => true,
            ExpandedTypeSelection::Type(iri) => type_iris.iter().any(|candidate| candidate.as_ref() == iri.as_str()),
            ExpandedTypeSelection::And(children) => children.iter().all(|child| child.matches(type_iris)),
            ExpandedTypeSelection::Or(children) => children.iter().any(|child| child.matches(type_iris)),
        }
    }

    /// The operands of the expression's top-level disjunction (clause 4.17 `|`
    /// or `,`), in the order written, or the expression itself when it is no
    /// disjunction.
    ///
    /// Clause 4.17 nests no disjunction inside another group, so each
    /// alternative of a parsed expression is one type, a parenthesised
    /// conjunction, or the wildcard standing alone.
    #[must_use]
    pub const fn alternatives(&self) -> &[ExpandedTypeSelection] {
        match self {
            ExpandedTypeSelection::Or(children) => children.as_slice(),
            ExpandedTypeSelection::Wildcard | ExpandedTypeSelection::Type(_) | ExpandedTypeSelection::And(_) => slice::from_ref(self),
        }
    }

    /// The type IRIs the expression names, in order of appearance.
    ///
    /// The wildcard names none. A caller that must hand a flat list of types to
    /// code that does not read the boolean structure uses this, knowing it
    /// over-approximates a conjunction.
    #[must_use]
    pub fn named_types(&self) -> Vec<IriBuf> {
        match self {
            ExpandedTypeSelection::Wildcard => Vec::new(),
            ExpandedTypeSelection::Type(iri) => vec![iri.clone()],
            ExpandedTypeSelection::And(children) | ExpandedTypeSelection::Or(children) => {
                children.iter().flat_map(ExpandedTypeSelection::named_types).collect()
            }
        }
    }

    /// Convert an expanded expression into the serialisable expression shape.
    ///
    /// Full IRIs satisfy the same clause-4.6.2 name grammar as compact type
    /// names. This conversion lets a stored resource retain the clause-4.17
    /// wire shape after every leaf has crossed the JSON-LD expansion boundary.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TypeExprInvalidName`] when an expanded leaf IRI
    /// does not satisfy the clause 4.6.2 type-name grammar.
    pub fn into_raw(self) -> Result<WrittenTypeSelection, QueryError> {
        match self {
            ExpandedTypeSelection::Wildcard => Ok(WrittenTypeSelection::Wildcard),
            ExpandedTypeSelection::Type(iri) => iri
                .as_str()
                .parse()
                .map(WrittenTypeSelection::Type)
                .map_err(|source| QueryError::TypeExprInvalidName { position: 0, source }),
            ExpandedTypeSelection::And(children) => children
                .into_iter()
                .map(ExpandedTypeSelection::into_raw)
                .collect::<Result<Vec<_>, _>>()
                .map(WrittenTypeSelection::And),
            ExpandedTypeSelection::Or(children) => children
                .into_iter()
                .map(ExpandedTypeSelection::into_raw)
                .collect::<Result<Vec<_>, _>>()
                .map(WrittenTypeSelection::Or),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_selection::{expanded_type_selection::ExpandedTypeSelection, written_type_selection::WrittenTypeSelection};
    use iri_rs::IriBuf;
    use std::slice;

    /// A stored expression reads back as the expanded expression it was
    /// written from, every leaf the IRI it holds.
    #[test]
    fn a_stored_expression_reads_back_as_the_expanded_expression_it_was_written_from() {
        let expanded = ExpandedTypeSelection::Or(vec![
            ExpandedTypeSelection::And(vec![
                ExpandedTypeSelection::Type(IriBuf::new("https://example.com/Home".to_owned()).expect("valid IRI")),
                ExpandedTypeSelection::Type(IriBuf::new("https://example.com/Vehicle".to_owned()).expect("valid IRI")),
            ]),
            ExpandedTypeSelection::Wildcard,
        ]);
        let stored = expanded.clone().into_raw().expect("every leaf is a type name");
        pretty_assertions::assert_eq!(stored.as_expanded().expect("every leaf is an IRI"), expanded);
    }

    #[test]
    fn an_expanded_expression_names_its_leaf_types_in_order() {
        let iri = |name: &str| IriBuf::new(format!("https://example.org/{name}")).expect("valid IRI");
        let expr = ExpandedTypeSelection::Or(vec![
            ExpandedTypeSelection::Type(iri("Vehicle")),
            ExpandedTypeSelection::And(vec![ExpandedTypeSelection::Type(iri("Home")), ExpandedTypeSelection::Type(iri("Asset"))]),
        ]);
        pretty_assertions::assert_eq!(expr.named_types(), vec![iri("Vehicle"), iri("Home"), iri("Asset")]);
        pretty_assertions::assert_eq!(ExpandedTypeSelection::Wildcard.named_types(), Vec::<IriBuf>::new());
    }

    #[test]
    fn the_alternatives_are_the_operands_of_the_top_level_disjunction() {
        let name = |value: &str| ExpandedTypeSelection::Type(IriBuf::new(format!("https://example.com/{value}")).expect("valid IRI"));
        let conjunction = ExpandedTypeSelection::And(vec![name("Home"), name("Vehicle")]);
        pretty_assertions::assert_eq!(name("Vehicle").alternatives(), [name("Vehicle")]);
        pretty_assertions::assert_eq!(
            ExpandedTypeSelection::Or(vec![name("Building"), name("Tower")]).alternatives(),
            [name("Building"), name("Tower")]
        );
        pretty_assertions::assert_eq!(
            ExpandedTypeSelection::Or(vec![conjunction.clone(), name("Motorhome")]).alternatives(),
            [conjunction.clone(), name("Motorhome")]
        );
        pretty_assertions::assert_eq!(conjunction.alternatives(), slice::from_ref(&conjunction));
        pretty_assertions::assert_eq!(ExpandedTypeSelection::Wildcard.alternatives(), [ExpandedTypeSelection::Wildcard]);
    }

    fn iri(raw: &str) -> ExpandedTypeSelection {
        ExpandedTypeSelection::Type(raw.parse().unwrap_or_else(|error| panic!("{raw} must be an IRI: {error}")))
    }

    #[test]
    fn an_expanded_alternation_matches_whole_iris_only() {
        // Clause 4.17 over the expanded form: the leaves are IRIs, and a
        // longer IRI that merely ends with one of them is a different type.
        let expr = ExpandedTypeSelection::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        assert!(expr.matches(&["https://example.com/Building"]));
        assert!(expr.matches(&["https://example.com/Tower"]));
        assert!(!expr.matches(&["https://example.com/MyBuilding"]));
        assert!(!expr.matches(&["https://example.com/Vehicle"]));
    }

    #[test]
    fn an_expanded_conjunction_needs_every_type_present() {
        let expr = ExpandedTypeSelection::And(vec![iri("https://example.com/Home"), iri("https://example.com/Vehicle")]);
        assert!(expr.matches(&["https://example.com/Home", "https://example.com/Vehicle"]));
        assert!(!expr.matches(&["https://example.com/Home"]));
    }

    #[test]
    fn the_expanded_and_compact_readings_of_one_expression_agree_in_structure() {
        // The two ASTs are the same clause 4.17 tree either side of expansion,
        // so a shape that matches compactly must match once expanded.
        let compact: WrittenTypeSelection = "Building,Tower".parse().expect("valid type expression");
        let expanded = ExpandedTypeSelection::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        assert!(compact.matches(&["Tower"]));
        assert!(expanded.matches(&["https://example.com/Tower"]));
        assert!(!compact.matches(&["Vehicle"]));
        assert!(!expanded.matches(&["https://example.com/Vehicle"]));
    }

    #[test]
    fn an_expanded_expression_converts_to_a_raw_tree_with_iri_leaves() {
        let expanded = ExpandedTypeSelection::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        let raw = expanded.into_raw().expect("expanded IRIs are valid NGSI-LD names");
        pretty_assertions::assert_eq!(raw.to_string(), "https://example.com/Building,https://example.com/Tower");
    }
}
