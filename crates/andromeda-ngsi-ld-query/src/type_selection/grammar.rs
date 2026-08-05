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

// The rendering walks a recursive tree and parenthesises every non-leaf child,
// which `derive_more::Display`'s per-variant format string cannot express.
impl fmt::Display for TypeExprRaw {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExprRaw::Type(name) => formatter.write_str(name),
            TypeExprRaw::And(children) => write_children(formatter, children, ';'),
            TypeExprRaw::Or(children) => write_children(formatter, children, ','),
        }
    }
}

/// Write a boolean node's children, separated and parenthesised so the result
/// parses back to the same tree regardless of operator precedence.
fn write_children(formatter: &mut fmt::Formatter<'_>, children: &[TypeExprRaw], separator: char) -> fmt::Result {
    for (index, child) in children.iter().enumerate() {
        if index > 0 {
            formatter.write_char(separator)?;
        }
        match child {
            TypeExprRaw::Type(name) => formatter.write_str(name)?,
            TypeExprRaw::And(_) | TypeExprRaw::Or(_) => write!(formatter, "({child})")?,
        }
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
    /// Matches entities whose types include this specific IRI.
    Type(IriBuf),
    /// Matches entities whose types satisfy all children (semicolon-separated).
    And(Vec<TypeExpr>),
    /// Matches entities whose types satisfy any child (comma/pipe-separated).
    Or(Vec<TypeExpr>),
}

#[cfg(test)]
mod tests {
    use crate::type_selection::grammar::TypeExprRaw;

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
}
