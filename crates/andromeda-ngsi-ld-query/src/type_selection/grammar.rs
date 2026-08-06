//! Abstract syntax of the entity type selection language (clause 4.17).
//!
//! Two shapes of the same tree: [`TypeExprRaw`] carries the compact names the
//! client wrote, [`TypeExpr`] carries the IRIs they expand to.

use crate::{error::QueryError, type_selection::parser};
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
    /// A single compact type name.
    Type(String),
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
            TypeExprRaw::Type(name) => types.iter().any(|candidate| candidate.as_ref() == name),
            TypeExprRaw::And(children) => children.iter().all(|child| child.matches(types)),
            TypeExprRaw::Or(children) => children.iter().any(|child| child.matches(types)),
        }
    }

    /// The single type name this expression selects, when it selects exactly
    /// one.
    ///
    /// A caller that indexes subscriptions by type can only do so for a leaf;
    /// a boolean expression has no single key to file it under.
    pub fn as_single_type(&self) -> Option<&str> {
        match self {
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

// The rendering walks a recursive tree and parenthesises the children the
// grammar would re-associate, which `derive_more::Display`'s per-variant format
// string cannot express.
//
// `|` and `,` are one operator in clause 4.17, and the tree does not record
// which of the two a client wrote, so a disjunction always renders as `,`. That
// is a normalisation of equivalent spellings, not a change of meaning; the
// structure is preserved exactly.
impl fmt::Display for TypeExprRaw {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExprRaw::Type(name) => formatter.write_str(name),
            TypeExprRaw::And(children) => write_children(formatter, children, ';'),
            TypeExprRaw::Or(children) => write_children(formatter, children, ','),
        }
    }
}

/// Write a boolean node's children, parenthesising any child the grammar would
/// otherwise re-associate.
///
/// Clause 4.17 makes `;` bind tighter than `,` and `|`, so a conjunction
/// directly inside a disjunction reads back unchanged and needs no parentheses.
/// Every other pairing does, including one operator nested inside itself, which
/// would otherwise flatten. Parenthesising every non-leaf child instead — which
/// is what this did — turned `A|B;C` into `A,(B;C)`, so a stored selector came
/// back carrying groups its client never wrote.
///
/// The rule matches the one in [`crate::filter::serialize`]; the two are not
/// shared because they render two independent specification grammars that
/// happen to agree on precedence today.
fn write_children(formatter: &mut fmt::Formatter<'_>, children: &[TypeExprRaw], separator: char) -> fmt::Result {
    for (index, child) in children.iter().enumerate() {
        if index > 0 {
            formatter.write_char(separator)?;
        }
        if needs_parentheses(child, separator) {
            write!(formatter, "({child})")?;
        } else {
            write!(formatter, "{child}")?;
        }
    }
    Ok(())
}

/// Whether a child of a node written with `separator` has to be parenthesised.
const fn needs_parentheses(child: &TypeExprRaw, separator: char) -> bool {
    match child {
        // A type name binds tighter than every operator.
        TypeExprRaw::Type(_) => false,
        // `;` binds tighter than `,`, so a conjunction inside a disjunction
        // reads back unchanged; a conjunction inside a conjunction would flatten.
        TypeExprRaw::And(_) => separator == ';',
        TypeExprRaw::Or(_) => true,
    }
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
            TypeExpr::Type(iri) => type_iris.iter().any(|candidate| candidate.as_ref() == iri.as_str()),
            TypeExpr::And(children) => children.iter().all(|child| child.matches(type_iris)),
            TypeExpr::Or(children) => children.iter().any(|child| child.matches(type_iris)),
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
    /// read back, so the rendering may not add structure the client did not
    /// write. It used to parenthesise every non-leaf child, turning `A|B;C`
    /// into `A,(B;C)`.
    #[test]
    fn the_rendering_adds_no_group_the_grammar_does_not_need() {
        let shapes = [
            ("Building", "Building"),
            ("Building|Tower", "Building,Tower"),
            ("(Building|Tower)", "Building,Tower"),
            ("A|B;C", "A,B;C"),
            ("(Home;Vehicle),Motorhome", "Home;Vehicle,Motorhome"),
            ("(A,B);C", "(A,B);C"),
            ("(A;B);C", "(A;B);C"),
            ("(A,B),C", "(A,B),C"),
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
        assert_eq!("Building".parse::<TypeExprRaw>().expect("valid").as_single_type(), Some("Building"));
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
}
