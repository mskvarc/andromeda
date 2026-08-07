//! The literal operator and conjunction tokens of the clause 4.9 `q` grammar.
//!
//! One owner for how each comparison and conjunction operator is spelled, so
//! the tokeniser, the serialiser and any restricted subset of the language —
//! such as the snapshot purge filter (clause 5.16.7) — cannot drift on the
//! token strings. The [`predicate`](crate::filter::predicate) operator types
//! render their clause 4.9 tokens from here.

/// `==`, the equality operator.
pub const EQUAL: &str = "==";
/// `!=`, the inequality operator.
pub const UNEQUAL: &str = "!=";
/// `>`, the greater-than operator.
pub const GREATER: &str = ">";
/// `>=`, the greater-than-or-equal operator.
pub const GREATER_EQ: &str = ">=";
/// `<`, the less-than operator.
pub const LESS: &str = "<";
/// `<=`, the less-than-or-equal operator.
pub const LESS_EQ: &str = "<=";
/// `;`, the conjunction (logical AND) operator.
pub const AND: &str = ";";
/// `|`, the disjunction (logical OR) operator.
pub const OR: &str = "|";
