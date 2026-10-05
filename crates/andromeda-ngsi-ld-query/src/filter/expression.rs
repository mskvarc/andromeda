//! The boolean structure of a parsed `q` filter (clause 4.9).

use crate::{
    error::QueryError,
    filter::{parser, predicate::Predicate, serialize},
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A parsed `q` filter expression tree.
///
/// Represents the boolean structure of an NGSI-LD query filter:
/// `And` and `Or` nodes combine sub-expressions; `Pred` leaves contain
/// individual predicates (existence checks, comparisons, pattern matches).
///
/// The wire form is the clause 4.9 expression itself, in both directions: a
/// resource that carries a filter — a Subscription (clause 5.2.12), a
/// `ContextSourceRegistration` (5.2.9), a Snapshot query (5.2.23) — stores the
/// text a client wrote and must give back an equivalent one. Parsing at
/// deserialization is what makes a malformed filter a rejected request instead
/// of a filter that silently matches nothing at first use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Expr {
    /// Conjunction: all sub-expressions must be true (`;` in NGSI-LD syntax).
    And(Vec<Expr>),
    /// Disjunction: at least one sub-expression must be true (`|` in NGSI-LD syntax).
    Or(Vec<Expr>),
    /// A leaf predicate.
    Pred(Predicate),
}

impl Expr {
    /// Whether any predicate in this filter traverses a linked entity — the
    /// clause 4.9 `{}` axis.
    ///
    /// A consumer evaluating a filter in memory needs to know: a traversal
    /// reaches an attribute of another entity, which has to be read before the
    /// filter can be decided.
    pub fn contains_linked_hop(&self) -> bool {
        match self {
            Expr::And(terms) | Expr::Or(terms) => terms.iter().any(Expr::contains_linked_hop),
            Expr::Pred(pred) => pred.path().is_linked(),
        }
    }

    /// The greatest number of Relationship hops any predicate in this filter
    /// crosses — the clause 4.9 `{}` axis, counted.
    ///
    /// Clause 5.7.2.4 bounds a filter's traversal by the request's `joinLevel`,
    /// and the bound applies to the deepest hop chain the filter contains: a
    /// shallow term standing beside a deep one must not hide the deep one.
    /// Zero when the filter stays inside the entity under evaluation.
    pub fn linked_depth(&self) -> u32 {
        match self {
            Expr::And(terms) | Expr::Or(terms) => terms.iter().map(Expr::linked_depth).max().unwrap_or(0),
            Expr::Pred(pred) => pred.path().linked_depth(),
        }
    }
}

impl FromStr for Expr {
    type Err = QueryError;

    fn from_str(raw: &str) -> Result<Expr, QueryError> {
        parser::parse(raw)
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it.
impl TryFrom<String> for Expr {
    type Error = QueryError;

    fn try_from(raw: String) -> Result<Expr, QueryError> {
        raw.parse()
    }
}

// The rendering walks a recursive tree and re-inserts the clause 4.9
// separators, which `derive_more::Display`'s per-variant format string cannot
// express.
impl fmt::Display for Expr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&serialize::serialize(self))
    }
}

// The wire form is produced by the `Display` above; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use it.
impl From<Expr> for String {
    fn from(expr: Expr) -> String {
        expr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::expression::Expr;

    #[test]
    fn a_filter_round_trips_through_its_clause_4_9_text() {
        // Clause 4.9: the wire form of a stored filter is the expression, so a
        // resource that is read back must yield a filter that parses to the
        // same tree.
        let raw = "speed>50;brandName==\"Mercedes\"";
        let json = serde_json::to_string(&raw.parse::<Expr>().expect("valid filter")).expect("serialises");
        pretty_assertions::assert_eq!(json, format!("{raw:?}"));
        let decoded: Expr = serde_json::from_str(&json).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, raw.parse::<Expr>().expect("valid filter"));
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
            let expr = raw.parse::<Expr>().expect("valid filter");
            pretty_assertions::assert_eq!(expr.linked_depth(), expected, "depth of {raw}");
        }
    }

    #[test]
    fn a_malformed_filter_is_rejected_when_the_resource_is_deserialized() {
        let error = serde_json::from_str::<Expr>("\"speed>\"").expect_err("must reject");
        assert!(error.to_string().contains("q expression"), "got {error}");
    }
}
