//! How clause 4.9's comparison and pattern operators are spelled in PostgreSQL.

use andromeda_ngsi_ld_query::filter::predicate::{CmpOp, PatternPolarity};

/// The SQL operator a clause 4.9 comparison renders to.
///
/// Shared by every rendering of clause 4.9, so a predicate cannot compile to
/// one operator against `current_attribute_values`, another against the payload and a
/// third against a registration's properties. It differs from the clause 4.9
/// token only at `==`, which SQL spells `=`.
pub const fn comparison(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

/// The POSIX regex operator a clause 4.9 pattern predicate renders to.
pub const fn pattern_match(polarity: PatternPolarity) -> &'static str {
    match polarity {
        PatternPolarity::Match => "~",
        PatternPolarity::NotMatch => "!~",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        use andromeda_ngsi_ld_query::filter::predicate::{OrderingOp, SetMembership};
        // The comparison primitive spells `==` as SQL `=`; every other
        // operator's SQL spelling matches its clause 4.9 token.
        assert_eq!(comparison(SetMembership::Within.as_cmp_op()), "=");
        assert_eq!(SetMembership::Within.ngsi_token(), "==");
        assert_eq!(comparison(SetMembership::Outside.as_cmp_op()), SetMembership::Outside.ngsi_token());
        for op in [OrderingOp::Greater, OrderingOp::GreaterEq, OrderingOp::Less, OrderingOp::LessEq] {
            assert_eq!(comparison(op.as_cmp_op()), op.ngsi_token(), "{op:?}");
        }
    }

    #[test]
    fn a_pattern_predicate_renders_the_posix_operator_of_its_polarity() {
        assert_eq!(pattern_match(PatternPolarity::Match), "~");
        assert_eq!(pattern_match(PatternPolarity::NotMatch), "!~");
    }
}
