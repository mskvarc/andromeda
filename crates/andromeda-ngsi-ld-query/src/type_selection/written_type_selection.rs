//! An entity type selection (clause 4.17) as the client wrote it, its type names compact.

use crate::{
    query_error::QueryError,
    term_expander::TermExpansionError,
    type_selection::{expanded_type_selection::ExpandedTypeSelection, parser},
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use iri_rs::IriBuf;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Write},
    str::FromStr,
};

/// Raw (pre-expansion) type expression AST node.
///
/// Leaf nodes contain compact type names that must be expanded to full IRIs
/// via the JSON-LD context before use in queries.
///
/// This is the form a stored resource carries: clause 5.2.33 puts a `type` in
/// an `EntitySelector`, and it reaches the broker before any `@context` has
/// been applied to it. The wire form is the clause 4.17 expression itself, so a
/// subscription that selected `Building|Tower` gives back an equivalent
/// expression rather than a regex someone rebuilt from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum WrittenTypeSelection {
    /// The clause-4.17 wildcard, selecting every entity type.
    Wildcard,
    /// A single compact type name, validated against the clause 4.6.2 grammar.
    Type(EntityTypeName),
    /// All children must match (semicolon-separated).
    And(Vec<WrittenTypeSelection>),
    /// Any child may match (comma/pipe-separated).
    Or(Vec<WrittenTypeSelection>),
}

impl WrittenTypeSelection {
    /// Whether a set of entity type names satisfies this expression.
    ///
    /// Names are compared whole, so the selector `Building` does not match an
    /// entity typed `MyBuilding`.
    pub fn matches<S: AsRef<str>>(&self, types: &[S]) -> bool {
        match self {
            WrittenTypeSelection::Wildcard => true,
            WrittenTypeSelection::Type(name) => types.iter().any(|candidate| candidate.as_ref() == name.as_str()),
            WrittenTypeSelection::And(children) => children.iter().all(|child| child.matches(types)),
            WrittenTypeSelection::Or(children) => children.iter().any(|child| child.matches(types)),
        }
    }

    /// The single type name this expression selects, when it selects exactly
    /// one.
    ///
    /// A caller that indexes subscriptions by type can only do so for a leaf;
    /// a boolean expression has no single key to file it under.
    #[must_use]
    pub const fn as_single_type(&self) -> Option<&EntityTypeName> {
        match self {
            WrittenTypeSelection::Type(name) => Some(name),
            // The wildcard selects every type and a boolean group selects a
            // combination of them, so neither names one.
            WrittenTypeSelection::Wildcard | WrittenTypeSelection::And(_) | WrittenTypeSelection::Or(_) => None,
        }
    }

    /// The type names the expression names, in order of appearance.
    ///
    /// The wildcard names none. As [`ExpandedTypeSelection::named_types`], this
    /// over-approximates a conjunction.
    #[must_use]
    pub fn named_types(&self) -> Vec<EntityTypeName> {
        match self {
            WrittenTypeSelection::Wildcard => Vec::new(),
            WrittenTypeSelection::Type(name) => vec![name.clone()],
            WrittenTypeSelection::And(children) | WrittenTypeSelection::Or(children) => children.iter().flat_map(WrittenTypeSelection::named_types).collect(),
        }
    }

    /// The expression a stored resource holds, read as the IRIs its leaves
    /// already are: the inverse of [`ExpandedTypeSelection::into_raw`].
    ///
    /// A resource stores its type expressions expanded, so every leaf of one
    /// read back is the absolute IRI the request's `@context` expanded it to.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::TermExpansion`] when a leaf is no absolute IRI,
    /// which an expression expanded before it was stored never holds.
    pub fn as_expanded(&self) -> Result<ExpandedTypeSelection, QueryError> {
        match self {
            WrittenTypeSelection::Wildcard => Ok(ExpandedTypeSelection::Wildcard),
            WrittenTypeSelection::Type(name) => IriBuf::new(name.as_str().to_owned())
                .map(ExpandedTypeSelection::Type)
                .map_err(|_| QueryError::TermExpansion(TermExpansionError::NotAnIri)),
            WrittenTypeSelection::And(children) => children
                .iter()
                .map(WrittenTypeSelection::as_expanded)
                .collect::<Result<_, _>>()
                .map(ExpandedTypeSelection::And),
            WrittenTypeSelection::Or(children) => children
                .iter()
                .map(WrittenTypeSelection::as_expanded)
                .collect::<Result<_, _>>()
                .map(ExpandedTypeSelection::Or),
        }
    }

    /// One selector for `alternatives`: none for no alternative, the
    /// alternative itself for one, and their disjunction (clause 4.17) for
    /// several.
    #[must_use]
    pub fn any_of(alternatives: Vec<WrittenTypeSelection>) -> Option<WrittenTypeSelection> {
        let mut alternatives = alternatives.into_iter();
        match (alternatives.next(), alternatives.next()) {
            (None, None | Some(_)) => None,
            (Some(only), None) => Some(only),
            (Some(first), Some(second)) => Some(WrittenTypeSelection::Or([first, second].into_iter().chain(alternatives).collect())),
        }
    }
}

impl FromStr for WrittenTypeSelection {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<WrittenTypeSelection, QueryError> {
        parser::parse(raw)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for WrittenTypeSelection {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<WrittenTypeSelection, QueryError> {
        raw.parse()
    }
}

/// Where a node sits while it is rendered.
///
/// Clause 4.17 admits a conjunction only in parentheses and a top-level
/// disjunction without them, so a disjunction needs enclosing precisely when it
/// is nested inside another group.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nesting {
    /// The whole expression, where a disjunction stands unparenthesised.
    Top,
    /// A child of a `;` or `,` group, where a nested disjunction is enclosed.
    InGroup,
}

// `|` and `,` are one operator in clause 4.17, and the tree does not record
// which of the two a client wrote, so a disjunction always renders as `,`. That
// is a normalisation of equivalent spellings, not a change of meaning.
//
// Clause 4.17's grammar is `OrEntityType = "(" EntityType *(";" EntityType) ")"
// / EntityType`: a conjunction is legal *only* parenthesised, with no
// unparenthesised form and no precedence that would let one be inferred. So an
// `And` group is always wrapped: a bare `Home;Vehicle` is text outside the
// grammar that a conforming parser rejects, where `(Home;Vehicle)` is not.
impl fmt::Display for WrittenTypeSelection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(self, formatter, Nesting::Top)
    }
}

/// Render `node`, enclosing a conjunction always and a disjunction only when it
/// is nested inside another group (clause 4.17).
fn render(node: &WrittenTypeSelection, formatter: &mut fmt::Formatter<'_>, nesting: Nesting) -> fmt::Result {
    match node {
        WrittenTypeSelection::Wildcard => formatter.write_char('*'),
        WrittenTypeSelection::Type(name) => formatter.write_str(name.as_str()),
        WrittenTypeSelection::And(children) => {
            formatter.write_char('(')?;
            write_separated(formatter, children, ';')?;
            formatter.write_char(')')
        }
        WrittenTypeSelection::Or(children) => {
            let enclose = nesting == Nesting::InGroup;
            if enclose {
                formatter.write_char('(')?;
            }
            write_separated(formatter, children, ',')?;
            if enclose {
                formatter.write_char(')')?;
            }
            Ok(())
        }
    }
}

/// Write a group's children separated by `separator`, each rendered as nested.
fn write_separated(formatter: &mut fmt::Formatter<'_>, children: &[WrittenTypeSelection], separator: char) -> fmt::Result {
    for (index, child) in children.iter().enumerate() {
        if index > 0 {
            formatter.write_char(separator)?;
        }
        render(child, formatter, Nesting::InGroup)?;
    }
    Ok(())
}

// The wire form is produced by the `Display` above; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl From<WrittenTypeSelection> for String {
    fn from(expr: WrittenTypeSelection) -> String {
        expr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::type_selection::written_type_selection::WrittenTypeSelection;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;

    #[test]
    fn a_type_expression_round_trips_through_its_clause_4_17_text() {
        for raw in ["Building", "Building,Tower", "(Building;Vehicle)", "(Home;Vehicle),Motorhome"] {
            let parsed: WrittenTypeSelection = raw.parse().expect("valid type expression");
            let reparsed: WrittenTypeSelection = parsed.to_string().parse().expect("re-parses");
            pretty_assertions::assert_eq!(parsed, reparsed, "{raw}");
        }
    }

    /// A leaf that is no absolute IRI was never expanded, so it is refused
    /// rather than read as an IRI it does not name.
    #[test]
    fn a_compact_leaf_is_no_expanded_expression() {
        let compact: WrittenTypeSelection = "Vehicle".parse().expect("valid type expression");
        assert!(compact.as_expanded().is_err());
    }

    #[test]
    fn an_alternation_matches_whole_names_only() {
        // Names are compared whole: `Building` does not match `MyBuilding`.
        let expr: WrittenTypeSelection = "Building|Tower".parse().expect("valid type expression");
        assert!(expr.matches(&["Building"]));
        assert!(expr.matches(&["Tower"]));
        assert!(!expr.matches(&["MyBuilding"]));
        assert!(!expr.matches(&["Vehicle"]));
    }

    /// A stored selector is re-serialised whenever the resource carrying it is
    /// read back, and clause 4.17 admits a conjunction only in parentheses:
    /// `OrEntityType = "(" EntityType *(";" EntityType) ")" / EntityType`. So a
    /// conjunction always renders parenthesised, while a top-level disjunction
    /// stays bare.
    #[test]
    fn a_conjunction_always_renders_parenthesised_per_clause_4_17() {
        let shapes = [
            ("Building", "Building"),
            ("Building,Tower", "Building,Tower"),
            ("Building|Tower", "Building,Tower"),
            ("(Home;Vehicle)", "(Home;Vehicle)"),
            ("(Home;Vehicle),Motorhome", "(Home;Vehicle),Motorhome"),
            ("Motorhome,(Home;Vehicle)", "Motorhome,(Home;Vehicle)"),
        ];
        for (input, expected) in shapes {
            let parsed: WrittenTypeSelection = input.parse().unwrap_or_else(|error| panic!("{input} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, expected, "rendering of {input}");
            let reparsed: WrittenTypeSelection = rendered.parse().unwrap_or_else(|error| panic!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {input}");
        }
    }

    #[test]
    fn a_conjunction_needs_every_type_present() {
        let expr: WrittenTypeSelection = "(Home;Vehicle)".parse().expect("valid type expression");
        assert!(expr.matches(&["Home", "Vehicle"]));
        assert!(!expr.matches(&["Home"]));
    }

    #[test]
    fn any_of_no_alternative_is_no_selector_and_of_one_is_that_alternative() {
        let parsed = |raw: &str| raw.parse::<WrittenTypeSelection>().expect("valid type expression");
        pretty_assertions::assert_eq!(WrittenTypeSelection::any_of(Vec::new()), None);
        pretty_assertions::assert_eq!(WrittenTypeSelection::any_of(vec![parsed("(Home;Vehicle)")]), Some(parsed("(Home;Vehicle)")));
        pretty_assertions::assert_eq!(
            WrittenTypeSelection::any_of(vec![parsed("Building"), parsed("(Home;Vehicle)"), parsed("Tower")]),
            Some(parsed("Building|(Home;Vehicle)|Tower"))
        );
    }

    #[test]
    fn a_raw_expression_names_its_leaf_types_in_order() {
        let name = |raw: &str| raw.parse::<EntityTypeName>().expect("valid type name");
        let expr = "Vehicle,(Home;Asset)".parse::<WrittenTypeSelection>().expect("valid");
        pretty_assertions::assert_eq!(expr.named_types(), vec![name("Vehicle"), name("Home"), name("Asset")]);
        pretty_assertions::assert_eq!(WrittenTypeSelection::Wildcard.named_types(), Vec::<EntityTypeName>::new());
    }

    #[test]
    fn only_a_leaf_names_a_single_type() {
        assert_eq!(
            "Building"
                .parse::<WrittenTypeSelection>()
                .expect("valid")
                .as_single_type()
                .map(EntityTypeName::as_str),
            Some("Building")
        );
        assert_eq!("A,B".parse::<WrittenTypeSelection>().expect("valid").as_single_type(), None);
    }

    #[test]
    fn a_malformed_type_expression_is_rejected_when_the_resource_is_deserialized() {
        assert!(serde_json::from_str::<WrittenTypeSelection>("\"(Building\"").is_err());
    }
}
