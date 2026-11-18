//! The Query Terms of a `q` filter and the operators they carry (clause 4.9).

use crate::{
    filter::{attribute_path::AttributePath, equality_value::EqualityValue, operator_spelling, reading::order_relation::OrderRelation, scalar::Scalar},
    pattern::Pattern,
};

/// A single query term within a `q` filter.
///
/// Clause 4.9 Table 4.9-1 admits a range or a value list only after `==`/`!=`;
/// the four ordering operators take a single scalar. The two comparison
/// variants encode that split at the type level, so a range or a list can never
/// attach to an ordering operator and no rendering has to re-check eligibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryTerm {
    /// Attribute existence check (e.g. `temperature`).
    Exists(AttributePath),
    /// Equality comparison (`==`/`!=`) of an attribute value against a scalar,
    /// a range, or a value list.
    Equality {
        /// The attribute path to compare.
        path: AttributePath,
        /// Whether the value must lie inside (`==`) or outside (`!=`) the operand.
        membership: SetMembership,
        /// The equality value: a scalar, a range or a value list.
        value: EqualityValue,
    },
    /// Ordering comparison (`>`/`>=`/`<`/`<=`) of an attribute value against a
    /// single scalar.
    Ordering {
        /// The attribute path to compare.
        path: AttributePath,
        /// The ordering operator.
        operator: OrderingOperator,
        /// The single scalar operand; clause 4.9 admits no compound operand here.
        value: Scalar,
    },
    /// Regex match: attribute value must match the pattern (e.g. `name~="A.*"`).
    MatchPattern {
        /// The attribute path to match against.
        path: AttributePath,
        /// The regex pattern, validated when the filter was parsed.
        pattern: Pattern,
    },
    /// Negated regex match: attribute value must not match the pattern (e.g. `name!~="A.*"`).
    NotMatchPattern {
        /// The attribute path to match against.
        path: AttributePath,
        /// The regex pattern that must not match, validated when the filter
        /// was parsed.
        pattern: Pattern,
    },
}

impl QueryTerm {
    /// The attribute path this query term compares along.
    ///
    /// Every variant carries one, so a consumer that only has to walk the path
    /// (a SQL compiler descending the linked-entity hops, for instance) reads it
    /// here rather than re-matching all five variants.
    #[must_use]
    pub const fn path(&self) -> &AttributePath {
        match self {
            QueryTerm::Exists(path)
            | QueryTerm::Equality { path, .. }
            | QueryTerm::Ordering { path, .. }
            | QueryTerm::MatchPattern { path, .. }
            | QueryTerm::NotMatchPattern { path, .. } => path,
        }
    }

    /// The attribute path this query term compares along, mutably.
    pub const fn path_mut(&mut self) -> &mut AttributePath {
        match self {
            QueryTerm::Exists(path)
            | QueryTerm::Equality { path, .. }
            | QueryTerm::Ordering { path, .. }
            | QueryTerm::MatchPattern { path, .. }
            | QueryTerm::NotMatchPattern { path, .. } => path,
        }
    }
}

/// The four clause 4.9 ordering operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingOperator {
    /// Greater than (`>`).
    Greater,
    /// Greater than or equal (`>=`).
    GreaterOrEqual,
    /// Less than (`<`).
    Less,
    /// Less than or equal (`<=`).
    LessOrEqual,
}

impl OrderingOperator {
    /// Clause 4.9 token this comparison was written as.
    ///
    /// The inverse of what the parser consumes, used to serialise a filter back
    /// into query-string form.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            OrderingOperator::Greater => operator_spelling::GREATER,
            OrderingOperator::GreaterOrEqual => operator_spelling::GREATER_OR_EQUAL,
            OrderingOperator::Less => operator_spelling::LESS,
            OrderingOperator::LessOrEqual => operator_spelling::LESS_OR_EQUAL,
        }
    }

    /// The relation of the target to the operand the operator tests.
    #[must_use]
    pub const fn relation(self) -> OrderRelation {
        match self {
            OrderingOperator::Greater => OrderRelation::Greater,
            OrderingOperator::GreaterOrEqual => OrderRelation::GreaterOrEqual,
            OrderingOperator::Less => OrderRelation::Less,
            OrderingOperator::LessOrEqual => OrderRelation::LessOrEqual,
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
    pub const fn spelling(self) -> &'static str {
        match self {
            SetMembership::Within => operator_spelling::EQUAL,
            SetMembership::Outside => operator_spelling::UNEQUAL,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        query_term::{OrderingOperator, SetMembership},
        reading::order_relation::OrderRelation,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn set_membership_writes_the_equality_tokens() {
        assert_eq!(SetMembership::Within.spelling(), "==");
        assert_eq!(SetMembership::Outside.spelling(), "!=");
    }

    #[test]
    fn ordering_operators_write_their_token_and_test_their_relation() {
        for (operator, relation, token) in [
            (OrderingOperator::Greater, OrderRelation::Greater, ">"),
            (OrderingOperator::GreaterOrEqual, OrderRelation::GreaterOrEqual, ">="),
            (OrderingOperator::Less, OrderRelation::Less, "<"),
            (OrderingOperator::LessOrEqual, OrderRelation::LessOrEqual, "<="),
        ] {
            assert_eq!(operator.relation(), relation, "{operator:?}");
            assert_eq!(operator.spelling(), token, "{operator:?}");
        }
    }
}
