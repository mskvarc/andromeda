//! Leaf predicates of a `q` filter and the operators they carry (clause 4.9).

use crate::{
    filter::{
        path::QueryPath,
        reading::operand::Comparison,
        scalar::{CompareValue, Scalar},
        token,
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_value_order::string_order::string_order;
use std::cmp::Ordering;

/// A single filter predicate within a `q` expression.
///
/// Clause 4.9 Table 4.9-1 admits a range or a value list only after `==`/`!=`;
/// the four ordering operators take a single scalar. The two comparison
/// variants encode that split at the type level, so a range or a list can never
/// attach to an ordering operator and no rendering has to re-check eligibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// Attribute existence check (e.g. `temperature`).
    Exists(QueryPath),
    /// Equality comparison (`==`/`!=`) of an attribute value against a scalar,
    /// a range, or a value list.
    Equality {
        /// The attribute path to compare.
        path: QueryPath,
        /// Whether the value must lie inside (`==`) or outside (`!=`) the operand.
        membership: SetMembership,
        /// The right-hand side value(s).
        rhs: CompareValue,
    },
    /// Ordering comparison (`>`/`>=`/`<`/`<=`) of an attribute value against a
    /// single scalar.
    Ordering {
        /// The attribute path to compare.
        path: QueryPath,
        /// The ordering operator.
        op: OrderingOp,
        /// The single scalar operand; clause 4.9 admits no compound operand here.
        rhs: Scalar,
    },
    /// Regex match: attribute value must match the pattern (e.g. `name~="A.*"`).
    MatchPattern {
        /// The attribute path to match against.
        path: QueryPath,
        /// The regex pattern, validated when the filter was parsed.
        pattern: Pattern,
    },
    /// Negated regex match: attribute value must not match the pattern (e.g. `name!~="A.*"`).
    NotMatchPattern {
        /// The attribute path to match against.
        path: QueryPath,
        /// The regex pattern that must not match, validated when the filter
        /// was parsed.
        pattern: Pattern,
    },
}

impl Predicate {
    /// The attribute axis this predicate compares along.
    ///
    /// Every variant carries one, so a consumer that only has to walk the axis
    /// (a SQL compiler descending the linked-entity hops, for instance) reads it
    /// here rather than re-matching all five variants.
    #[must_use]
    pub const fn path(&self) -> &QueryPath {
        match self {
            Predicate::Exists(path)
            | Predicate::Equality { path, .. }
            | Predicate::Ordering { path, .. }
            | Predicate::MatchPattern { path, .. }
            | Predicate::NotMatchPattern { path, .. } => path,
        }
    }

    /// The attribute axis this predicate compares along, mutably.
    pub const fn path_mut(&mut self) -> &mut QueryPath {
        match self {
            Predicate::Exists(path)
            | Predicate::Equality { path, .. }
            | Predicate::Ordering { path, .. }
            | Predicate::MatchPattern { path, .. }
            | Predicate::NotMatchPattern { path, .. } => path,
        }
    }
}

/// The internal comparison primitive: any of the six clause 4.9 comparison
/// operators.
///
/// Not carried by [`Predicate`]: its two comparison variants carry the
/// parser-facing [`SetMembership`] and [`OrderingOp`] instead. This is the
/// operator an evaluator or a backend renders a single scalar comparison with,
/// including the internal `>=`/`<=`/`=` a range or a value list expands into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// Equal (`=`).
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
    /// Whether a target ordered `ord` against the operand satisfies the
    /// comparison.
    #[must_use]
    pub const fn holds(self, ord: Ordering) -> bool {
        match self {
            CmpOp::Equal => ord.is_eq(),
            CmpOp::Unequal => !ord.is_eq(),
            CmpOp::Greater => ord.is_gt(),
            CmpOp::GreaterEq => ord.is_ge(),
            CmpOp::Less => ord.is_lt(),
            CmpOp::LessEq => ord.is_le(),
        }
    }

    /// Whether the comparison holds between the text strings `lhs` and `rhs`,
    /// ordered by the relation [`Comparison::string_relation`] gives the
    /// operator's comparison.
    #[must_use]
    pub fn holds_for_strings(self, lhs: &str, rhs: &str) -> bool {
        let comparison = match self {
            CmpOp::Equal | CmpOp::Unequal => Comparison::Equality,
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => Comparison::Order,
        };
        self.holds(string_order(lhs, rhs, comparison.string_relation()))
    }
}

/// The four clause 4.9 ordering operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingOp {
    /// Greater than (`>`).
    Greater,
    /// Greater than or equal (`>=`).
    GreaterEq,
    /// Less than (`<`).
    Less,
    /// Less than or equal (`<=`).
    LessEq,
}

impl OrderingOp {
    /// Clause 4.9 token this comparison was written as.
    ///
    /// The inverse of what the parser consumes, used to serialise a filter back
    /// into query-string form.
    #[must_use]
    pub const fn ngsi_token(self) -> &'static str {
        match self {
            OrderingOp::Greater => token::GREATER,
            OrderingOp::GreaterEq => token::GREATER_EQ,
            OrderingOp::Less => token::LESS,
            OrderingOp::LessEq => token::LESS_EQ,
        }
    }

    /// The comparison primitive this ordering operator renders as.
    #[must_use]
    pub const fn as_cmp_op(self) -> CmpOp {
        match self {
            OrderingOp::Greater => CmpOp::Greater,
            OrderingOp::GreaterEq => CmpOp::GreaterEq,
            OrderingOp::Less => CmpOp::Less,
            OrderingOp::LessEq => CmpOp::LessEq,
        }
    }
}

/// Whether a compound operand (a range or a value list) is asserted or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetMembership {
    /// `==`: the attribute value lies inside the range or the list.
    Within,
    /// `!=`: it lies outside.
    Outside,
}

impl SetMembership {
    /// Clause 4.9 token this equality comparison was written as.
    #[must_use]
    pub const fn ngsi_token(self) -> &'static str {
        match self {
            SetMembership::Within => token::EQUAL,
            SetMembership::Outside => token::UNEQUAL,
        }
    }

    /// The comparison primitive a single scalar operand renders as under this
    /// membership: `==` for [`SetMembership::Within`], `!=` for
    /// [`SetMembership::Outside`].
    #[must_use]
    pub const fn as_cmp_op(self) -> CmpOp {
        match self {
            SetMembership::Within => CmpOp::Equal,
            SetMembership::Outside => CmpOp::Unequal,
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn set_membership_maps_to_the_equality_comparison_primitive() {
        // Clause 4.9 Table 4.9-1: `==`/`!=` are the only operators over a
        // compound operand, and a single scalar under them compares by equality.
        assert_eq!(SetMembership::Within.as_cmp_op(), CmpOp::Equal);
        assert_eq!(SetMembership::Outside.as_cmp_op(), CmpOp::Unequal);
        assert_eq!(SetMembership::Within.ngsi_token(), "==");
        assert_eq!(SetMembership::Outside.ngsi_token(), "!=");
    }

    #[test]
    fn ordering_operators_map_to_their_comparison_primitive_and_token() {
        // Clause 4.9 Table 4.9-1: the four ordering operators take a single
        // scalar, which a backend renders with the matching primitive.
        for (op, primitive, token) in [
            (OrderingOp::Greater, CmpOp::Greater, ">"),
            (OrderingOp::GreaterEq, CmpOp::GreaterEq, ">="),
            (OrderingOp::Less, CmpOp::Less, "<"),
            (OrderingOp::LessEq, CmpOp::LessEq, "<="),
        ] {
            assert_eq!(op.as_cmp_op(), primitive, "{op:?}");
            assert_eq!(op.ngsi_token(), token, "{op:?}");
        }
    }

    #[test]
    fn an_equality_compares_strings_by_code_point_and_an_ordering_by_collation() {
        assert!(!CmpOp::Equal.holds_for_strings("\u{e9}", "e\u{301}"));
        assert!(CmpOp::Unequal.holds_for_strings("\u{e9}", "e\u{301}"));
        assert!(CmpOp::Greater.holds_for_strings("\u{e9}", "e\u{301}"));
        assert!(CmpOp::Less.holds_for_strings("\u{e9}", "f"));
        assert!(CmpOp::LessEq.holds_for_strings("a", "B"));
        assert!(!CmpOp::GreaterEq.holds_for_strings("a", "B"));
    }
}
