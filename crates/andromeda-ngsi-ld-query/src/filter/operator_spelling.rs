//! The literal operators and conjunction marks of the clause 4.9 `q` grammar.
//!
//! One owner for how each comparison and conjunction operator is spelled, so
//! the parser, the serialiser and any restricted subset of the language,
//! such as the snapshot purge filter (clause 5.16.7), cannot drift on the
//! spellings. The [`query_term`](crate::filter::query_term) operator types
//! render their clause 4.9 operators from here.

/// `==`, the equality operator.
pub const EQUAL: &str = "==";
/// `!=`, the inequality operator.
pub const UNEQUAL: &str = "!=";
/// `>`, the greater-than operator.
pub const GREATER: &str = ">";
/// `>=`, the greater-than-or-equal operator.
pub const GREATER_OR_EQUAL: &str = ">=";
/// `<`, the less-than operator.
pub const LESS: &str = "<";
/// `<=`, the less-than-or-equal operator.
pub const LESS_OR_EQUAL: &str = "<=";
/// `~=`, the pattern-match operator.
pub const MATCH: &str = "~=";
/// `!~=`, the negated pattern-match operator.
pub const NOT_MATCH: &str = "!~=";
/// `..`, the range separator.
pub const RANGE: &str = "..";
/// `;`, the conjunction (logical AND) operator.
pub const AND: &str = ";";
/// `|`, the disjunction (logical OR) operator.
pub const OR: &str = "|";
