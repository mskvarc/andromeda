//! Abstract syntax of the entity type selection language (clause 4.17).
//!
//! Two shapes of the same tree: [`TypeExprRaw`] carries the compact names the
//! client wrote, [`TypeExpr`] carries the IRIs they expand to.

use crate::{error::QueryError, type_selection::parser};
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
/// subscription that selected `(Building|Tower)` gives back an equivalent
/// expression rather than a regex someone rebuilt from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum TypeExprRaw {
    /// The clause-4.17 wildcard, selecting every entity type.
    Wildcard,
    /// A single compact type name, validated against the clause 4.6.2 grammar.
    Type(EntityTypeName),
    /// All children must match (semicolon-separated).
    And(Vec<TypeExprRaw>),
    /// Any child may match (comma/pipe-separated).
    Or(Vec<TypeExprRaw>),
}

impl TypeExprRaw {
    /// Whether a set of entity type names satisfies this expression.
    ///
    /// Names are compared whole. Subscription matching used to turn an
    /// alternation into an unanchored regex, under which the selector `Building`
    /// also matched an entity typed `MyBuilding`.
    pub fn matches<S: AsRef<str>>(&self, types: &[S]) -> bool {
        match self {
            TypeExprRaw::Wildcard => true,
            TypeExprRaw::Type(name) => types.iter().any(|candidate| candidate.as_ref() == name.as_str()),
            TypeExprRaw::And(children) => children.iter().all(|child| child.matches(types)),
            TypeExprRaw::Or(children) => children.iter().any(|child| child.matches(types)),
        }
    }

    /// The single type name this expression selects, when it selects exactly
    /// one.
    ///
    /// A caller that indexes subscriptions by type can only do so for a leaf;
    /// a boolean expression has no single key to file it under.
    pub const fn as_single_type(&self) -> Option<&EntityTypeName> {
        match self {
            TypeExprRaw::Wildcard => None,
            TypeExprRaw::Type(name) => Some(name),
            TypeExprRaw::And(_) | TypeExprRaw::Or(_) => None,
        }
    }
}

impl FromStr for TypeExprRaw {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<TypeExprRaw, QueryError> {
        parser::parse(raw)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for TypeExprRaw {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<TypeExprRaw, QueryError> {
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
// `And` group is always wrapped — a bare `Home;Vehicle` is text outside the
// grammar that a conforming peer broker rejects, where `(Home;Vehicle)` is not.
impl fmt::Display for TypeExprRaw {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        render(self, formatter, Nesting::Top)
    }
}

/// Render `node`, enclosing a conjunction always and a disjunction only when it
/// is nested inside another group (clause 4.17).
fn render(node: &TypeExprRaw, formatter: &mut fmt::Formatter<'_>, nesting: Nesting) -> fmt::Result {
    match node {
        TypeExprRaw::Wildcard => formatter.write_char('*'),
        TypeExprRaw::Type(name) => formatter.write_str(name.as_str()),
        TypeExprRaw::And(children) => {
            formatter.write_char('(')?;
            write_separated(formatter, children, ';')?;
            formatter.write_char(')')
        }
        TypeExprRaw::Or(children) => {
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
fn write_separated(formatter: &mut fmt::Formatter<'_>, children: &[TypeExprRaw], separator: char) -> fmt::Result {
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
impl From<TypeExprRaw> for String {
    fn from(expr: TypeExprRaw) -> String {
        expr.to_string()
    }
}

/// Boolean expression over entity type IRIs (NGSI-LD S4.17).
///
/// Supports AND (`And`), OR (`Or`), and single type (`Type`) nodes.
/// Constructed by parsing the `type` query parameter's selection language
/// grammar and expanding compact type names to full IRIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    /// The clause-4.17 wildcard, selecting every expanded entity type.
    Wildcard,
    /// Matches entities whose types include this specific IRI.
    Type(IriBuf),
    /// Matches entities whose types satisfy all children (semicolon-separated).
    And(Vec<TypeExpr>),
    /// Matches entities whose types satisfy any child (comma/pipe-separated).
    Or(Vec<TypeExpr>),
}

impl TypeExpr {
    /// Whether a set of expanded entity type IRIs satisfies this expression.
    ///
    /// The counterpart of [`TypeExprRaw::matches`] on the other side of
    /// expansion: the same clause 4.17 boolean structure, with leaves compared
    /// as IRIs rather than as compact names. A caller that has the expanded
    /// payload — a store that declined the type predicate, a notification
    /// matcher working on expanded JSON-LD — has IRIs and nothing to compact
    /// them against.
    pub fn matches<S: AsRef<str>>(&self, type_iris: &[S]) -> bool {
        match self {
            TypeExpr::Wildcard => true,
            TypeExpr::Type(iri) => type_iris.iter().any(|candidate| candidate.as_ref() == iri.as_str()),
            TypeExpr::And(children) => children.iter().all(|child| child.matches(type_iris)),
            TypeExpr::Or(children) => children.iter().any(|child| child.matches(type_iris)),
        }
    }

    /// Convert an expanded expression into the serialisable expression shape.
    ///
    /// Full IRIs satisfy the same clause-4.6.2 name grammar as compact type
    /// names. This conversion lets a stored resource retain the clause-4.17
    /// wire shape after every leaf has crossed the JSON-LD expansion boundary.
    pub fn into_raw(self) -> Result<TypeExprRaw, QueryError> {
        match self {
            TypeExpr::Wildcard => Ok(TypeExprRaw::Wildcard),
            TypeExpr::Type(iri) => {
                let value = iri.to_string();
                value
                    .parse()
                    .map(TypeExprRaw::Type)
                    .map_err(|_error| QueryError::TypeExprInvalidName { name: value, position: 0 })
            }
            TypeExpr::And(children) => children
                .into_iter()
                .map(TypeExpr::into_raw)
                .collect::<Result<Vec<_>, _>>()
                .map(TypeExprRaw::And),
            TypeExpr::Or(children) => children.into_iter().map(TypeExpr::into_raw).collect::<Result<Vec<_>, _>>().map(TypeExprRaw::Or),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_selection::grammar::{TypeExpr, TypeExprRaw};

    #[test]
    fn a_type_expression_round_trips_through_its_clause_4_17_text() {
        for raw in ["Building", "Building,Tower", "Building;Vehicle", "(Home;Vehicle),Motorhome"] {
            let parsed: TypeExprRaw = raw.parse().expect("valid type expression");
            let reparsed: TypeExprRaw = parsed.to_string().parse().expect("re-parses");
            pretty_assertions::assert_eq!(parsed, reparsed, "{raw}");
        }
    }

    #[test]
    fn an_alternation_matches_whole_names_only() {
        // The regex reading this replaces was unanchored, so `Building` also
        // matched an entity typed `MyBuilding`.
        let expr: TypeExprRaw = "(Building|Tower)".parse().expect("valid type expression");
        assert!(expr.matches(&["Building"]));
        assert!(expr.matches(&["Tower"]));
        assert!(!expr.matches(&["MyBuilding"]));
        assert!(!expr.matches(&["Vehicle"]));
    }

    /// A stored selector is re-serialised whenever the resource carrying it is
    /// read back, and clause 4.17 admits a conjunction only in parentheses:
    /// `OrEntityType = "(" EntityType *(";" EntityType) ")" / EntityType`. So a
    /// conjunction always renders parenthesised — a bare `Home;Vehicle` is
    /// outside the grammar a conforming peer broker accepts — while a top-level
    /// disjunction, which the grammar admits unparenthesised, stays bare.
    #[test]
    fn a_conjunction_always_renders_parenthesised_per_clause_4_17() {
        let shapes = [
            ("Building", "Building"),
            ("Building,Tower", "Building,Tower"),
            ("Building|Tower", "Building,Tower"),
            ("(Building|Tower)", "Building,Tower"),
            ("Home;Vehicle", "(Home;Vehicle)"),
            ("(Home;Vehicle)", "(Home;Vehicle)"),
            ("(Home;Vehicle),Motorhome", "(Home;Vehicle),Motorhome"),
            ("Motorhome,(Home;Vehicle)", "Motorhome,(Home;Vehicle)"),
        ];
        for (input, expected) in shapes {
            let parsed: TypeExprRaw = input.parse().unwrap_or_else(|error| unreachable!("{input} must parse: {error}"));
            let rendered = parsed.to_string();
            pretty_assertions::assert_eq!(rendered, expected, "rendering of {input}");
            let reparsed: TypeExprRaw = rendered.parse().unwrap_or_else(|error| unreachable!("{rendered} must reparse: {error}"));
            pretty_assertions::assert_eq!(reparsed, parsed, "round trip of {input}");
        }
    }

    #[test]
    fn a_conjunction_needs_every_type_present() {
        let expr: TypeExprRaw = "Home;Vehicle".parse().expect("valid type expression");
        assert!(expr.matches(&["Home", "Vehicle"]));
        assert!(!expr.matches(&["Home"]));
    }

    #[test]
    fn only_a_leaf_names_a_single_type() {
        assert_eq!(
            "Building".parse::<TypeExprRaw>().expect("valid").as_single_type().map(|name| name.as_str()),
            Some("Building")
        );
        assert_eq!("A,B".parse::<TypeExprRaw>().expect("valid").as_single_type(), None);
    }

    #[test]
    fn a_malformed_type_expression_is_rejected_when_the_resource_is_deserialized() {
        assert!(serde_json::from_str::<TypeExprRaw>("\"(Building\"").is_err());
    }

    fn iri(raw: &str) -> TypeExpr {
        TypeExpr::Type(raw.parse().unwrap_or_else(|error| unreachable!("{raw} must be an IRI: {error}")))
    }

    #[test]
    fn an_expanded_alternation_matches_whole_iris_only() {
        // Clause 4.17 over the expanded form: the leaves are IRIs, and a
        // longer IRI that merely ends with one of them is a different type.
        let expr = TypeExpr::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        assert!(expr.matches(&["https://example.com/Building"]));
        assert!(expr.matches(&["https://example.com/Tower"]));
        assert!(!expr.matches(&["https://example.com/MyBuilding"]));
        assert!(!expr.matches(&["https://example.com/Vehicle"]));
    }

    #[test]
    fn an_expanded_conjunction_needs_every_type_present() {
        let expr = TypeExpr::And(vec![iri("https://example.com/Home"), iri("https://example.com/Vehicle")]);
        assert!(expr.matches(&["https://example.com/Home", "https://example.com/Vehicle"]));
        assert!(!expr.matches(&["https://example.com/Home"]));
    }

    #[test]
    fn the_expanded_and_compact_readings_of_one_expression_agree_in_structure() {
        // The two ASTs are the same clause 4.17 tree either side of expansion,
        // so a shape that matches compactly must match once expanded.
        let compact: TypeExprRaw = "Building,Tower".parse().expect("valid type expression");
        let expanded = TypeExpr::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        assert!(compact.matches(&["Tower"]));
        assert!(expanded.matches(&["https://example.com/Tower"]));
        assert!(!compact.matches(&["Vehicle"]));
        assert!(!expanded.matches(&["https://example.com/Vehicle"]));
    }

    #[test]
    fn an_expanded_expression_converts_to_a_raw_tree_with_iri_leaves() {
        let expanded = TypeExpr::Or(vec![iri("https://example.com/Building"), iri("https://example.com/Tower")]);
        let raw = expanded.into_raw().expect("expanded IRIs are valid NGSI-LD names");
        pretty_assertions::assert_eq!(raw.to_string(), "https://example.com/Building,https://example.com/Tower");
    }
}
