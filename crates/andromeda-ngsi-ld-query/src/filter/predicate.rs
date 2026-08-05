//! Leaf predicates of a `q` filter and the operators they carry (clause 4.9).

use crate::{
    filter::{path::AttrPath, scalar::CompareValue},
    pattern::Pattern,
};

/// A single filter predicate within a `q` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// Attribute existence check (e.g. `temperature`).
    Exists(AttrPath),
    /// Comparison of an attribute value against a scalar, range, or value list.
    Compare {
        /// The attribute path to compare.
        path: AttrPath,
        /// The comparison operator.
        op: CmpOp,
        /// The right-hand side value(s).
        rhs: CompareValue,
    },
    /// Regex match: attribute value must match the pattern (e.g. `name~="A.*"`).
    MatchPattern {
        /// The attribute path to match against.
        path: AttrPath,
        /// The regex pattern, validated when the filter was parsed.
        pattern: Pattern,
    },
    /// Negated regex match: attribute value must not match the pattern (e.g. `name!~="A.*"`).
    NotMatchPattern {
        /// The attribute path to match against.
        path: AttrPath,
        /// The regex pattern that must not match, validated when the filter
        /// was parsed.
        pattern: Pattern,
    },
}

/// Comparison operators in `q` filter expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// Equal (`==`).
    Equal,
    /// Not equal (`!=`).
    Unequal,
    /// Greater than (`>`).
    Greater,
    /// Greater than or equal (`>=`).
    GreaterEq,
    /// Less than (`<`).
    Less,
    /// Less than or equal (`<=`).
    LessEq,
}

impl CmpOp {
    /// SQL operator this comparison renders to.
    ///
    /// Shared by every SQL rendering of clause 4.9, so a predicate cannot
    /// compile to one operator against `attribute_current`, another against the
    /// payload and a third against a registration's properties.
    pub const fn sql_operator(self) -> &'static str {
        match self {
            CmpOp::Equal => "=",
            CmpOp::Unequal => "!=",
            CmpOp::Greater => ">",
            CmpOp::GreaterEq => ">=",
            CmpOp::Less => "<",
            CmpOp::LessEq => "<=",
        }
    }

    /// Clause 4.9 token this comparison was written as.
    ///
    /// The inverse of what the parser consumes, used to serialise a filter back
    /// into query-string form. Distinct from [`CmpOp::sql_operator`] only at
    /// `==`, which SQL spells `=`.
    pub const fn ngsi_token(self) -> &'static str {
        match self {
            CmpOp::Equal => "==",
            CmpOp::Unequal => "!=",
            CmpOp::Greater => ">",
            CmpOp::GreaterEq => ">=",
            CmpOp::Less => "<",
            CmpOp::LessEq => "<=",
        }
    }
}

/// Whether a compound operand — a range or a value list — is asserted or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetMembership {
    /// `==`: the attribute value lies inside the range or the list.
    Within,
    /// `!=`: it lies outside.
    Outside,
}

impl CmpOp {
    /// How this operator reads a compound operand, or `None` when clause 4.9
    /// admits none after it.
    ///
    /// Table 4.9-1 puts a range and a value list only after `==` and `!=`; the
    /// four ordering operators take a single scalar. Every rendering asks this
    /// rather than re-reading the clause, so an operator cannot become eligible
    /// against one backend and not another.
    pub const fn set_membership(self) -> Option<SetMembership> {
        match self {
            CmpOp::Equal => Some(SetMembership::Within),
            CmpOp::Unequal => Some(SetMembership::Outside),
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => None,
        }
    }
}

/// Whether a pattern predicate asserts a match or the absence of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternPolarity {
    /// `~=`
    Match,
    /// `!~=`
    NotMatch,
}

impl PatternPolarity {
    /// POSIX regex operator this polarity renders to.
    pub const fn operator(self) -> &'static str {
        match self {
            Self::Match => "~",
            Self::NotMatch => "!~",
        }
    }
}

/// Combine a presence test with a value test, as clause 4.9 requires.
///
/// Presence is conjoined in both polarities. Clause 4.9 states that the entity
/// "shall contain the target element" for a predicate to hold, and says so for
/// `notPatternOp` explicitly: `name!~="^A.*"` selects entities whose `name` is
/// present and does not match, not entities that have no `name` at all.
///
/// Every SQL rendering that tests a value builds its predicate here, which is
/// what stops one of them disjoining presence instead. One of them did: a
/// registration that simply lacked a property satisfied every negated `q`
/// predicate over it.
pub fn present_and(presence_sql: &str, value_sql: &str) -> String {
    format!("({presence_sql} AND {value_sql})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_equality_operators_admit_a_compound_operand() {
        // Clause 4.9 Table 4.9-1.
        assert_eq!(CmpOp::Equal.set_membership(), Some(SetMembership::Within));
        assert_eq!(CmpOp::Unequal.set_membership(), Some(SetMembership::Outside));
        for op in [CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            assert_eq!(op.set_membership(), None, "{op:?}");
        }
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        assert_eq!(present_and("x IS NOT NULL", "NOT (x ~ $1)"), "(x IS NOT NULL AND NOT (x ~ $1))");
    }
}
