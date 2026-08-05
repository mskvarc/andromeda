//! The boolean structure of a parsed `q` filter (clause 4.9).

use crate::filter::predicate::Predicate;

/// A parsed `q` filter expression tree.
///
/// Represents the boolean structure of an NGSI-LD query filter:
/// `And` and `Or` nodes combine sub-expressions; `Pred` leaves contain
/// individual predicates (existence checks, comparisons, pattern matches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Conjunction: all sub-expressions must be true (`;` in NGSI-LD syntax).
    And(Vec<Expr>),
    /// Disjunction: at least one sub-expression must be true (`|` in NGSI-LD syntax).
    Or(Vec<Expr>),
    /// A leaf predicate.
    Pred(Predicate),
}
