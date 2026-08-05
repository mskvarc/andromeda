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
/// ContextSourceRegistration (5.2.9), a Snapshot query (5.2.23) — stores the
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

    #[test]
    fn a_malformed_filter_is_rejected_when_the_resource_is_deserialized() {
        let error = serde_json::from_str::<Expr>("\"speed>\"").expect_err("must reject");
        assert!(error.to_string().contains("Q expression"), "got {error}");
    }
}
