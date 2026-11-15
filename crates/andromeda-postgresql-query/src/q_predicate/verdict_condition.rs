//! What a term makes of one target, from the condition its asserted test
//! holds under: the one statement of a complement, and of a target holding
//! several values.
//!
//! Clause 4.9 states each operator as a condition a matching Entity meets and
//! each negated operator as its complement over the same target, so every
//! rendering builds the asserted test alone and this module turns it into the
//! term. A complement is the failure of the asserted test over the targets in
//! its scope ([`ComplementScope`]): an unknown comparison, a value of a kind
//! the test does not compare with, is a failure, which clause 4.9 makes
//! "unequal" under `!=`, and only a String is a candidate of `!~=`, which
//! makes every other data type "not matching".

use andromeda_ngsi_ld_query::filter::reading::{
    array_rule::ArrayRule,
    value_test::{ComplementScope, ValueTest},
};
use andromeda_postgresql_seaquery::operator::negate_total;
use sea_query::{Condition, SimpleExpr};

/// The targets a complement ranges over, among those a probe reads.
#[derive(Debug, Clone)]
pub enum ComplementTarget {
    /// Every target the probe reads: the probe reads only a present one.
    Probed,
    /// The targets that satisfy the condition.
    Where(SimpleExpr),
}

/// The condition a term of `value` makes on one target, `positive` being the
/// condition under which the target passes the asserted test and `target`
/// naming the targets a complement of each scope ranges over.
#[must_use]
pub fn verdict_condition(value: &ValueTest, positive: Condition, target: impl FnOnce(ComplementScope) -> ComplementTarget) -> Condition {
    match value.complement_scope() {
        None => positive,
        Some(scope) => match target(scope) {
            ComplementTarget::Probed => Condition::all().add(does_not_hold(positive)),
            ComplementTarget::Where(target) => Condition::all().add(target).add(does_not_hold(positive)),
        },
    }
}

/// What a term whose rule for a target holding several values is `rule` makes
/// of such a target, `present` being the condition that the target holds
/// several values and `some_element` the condition that one of them passes
/// the test the elements are tested with; `None` where no such target passes.
#[must_use]
pub fn array_rule_condition(rule: ArrayRule, present: SimpleExpr, some_element: impl FnOnce() -> SimpleExpr) -> Option<Condition> {
    match rule {
        ArrayRule::Never => None,
        ArrayRule::Always => Some(Condition::all().add(present)),
        ArrayRule::SomeElement => Some(Condition::all().add(present).add(some_element())),
        ArrayRule::NoElement => Some(Condition::all().add(present).add(does_not_hold(Condition::all().add(some_element())))),
    }
}

/// Whether a condition does not hold, counting an unknown as a failure.
///
/// SQL's `NOT` propagates unknown: a comparison against a column the row
/// leaves empty is NULL, and `NOT NULL` is NULL, which the enclosing `WHERE`
/// drops, so a target the comparison could not even be evaluated against
/// would be a silent rejection.
fn does_not_hold(condition: Condition) -> SimpleExpr {
    negate_total(SimpleExpr::from(condition))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        lowered_filter::term_of,
        verdict_condition::{ComplementTarget, array_rule_condition, verdict_condition},
    };
    use andromeda_ngsi_ld_query::filter::reading::{
        array_rule::ArrayRule,
        term_reading::Test,
        value_test::{ComplementScope, ValueTest},
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};

    fn value_test(q: &str) -> ValueTest {
        let Test::Value(value) = term_of(q).test().clone() else {
            panic!("a value test");
        };
        value
    }

    fn column(name: &str) -> SimpleExpr {
        Expr::col(Alias::new(name))
    }

    fn positive() -> Condition {
        Condition::all().add(Expr::col(Alias::new("v")).eq(1))
    }

    fn render(condition: Condition) -> SelectStatement {
        Query::select().cond_where(condition).to_owned()
    }

    /// An asserted test is its positive condition; the complement names no
    /// scope it is not asked for.
    #[test]
    fn an_asserted_test_is_its_positive_condition() {
        let asserted = verdict_condition(&value_test("x==1"), positive(), |_| panic!("no complement"));
        assert_eq!(render(asserted), render(positive()));
    }

    /// Clause 4.9 Unequal: a target of another data type is unequal, so `!=`
    /// is the total negation of its equality over a probed target.
    #[test]
    fn an_unequal_term_is_the_total_negation_of_its_equality() {
        let mut scopes = Vec::new();
        let unequal = verdict_condition(&value_test("x!=1"), positive(), |scope| {
            scopes.push(scope);
            ComplementTarget::Probed
        });
        assert_eq!(scopes, [ComplementScope::AnyKind]);
        assert_statement_snapshot!("verdict-condition-an-unequal-term-is-the-total-negation-of-its-equality", render(unequal));
    }

    /// Clause 4.9 Do not match pattern: a target of another data type than
    /// String is not matching, so `!~=` negates the pattern over a String
    /// target alone.
    #[test]
    fn a_not_pattern_term_negates_the_pattern_over_a_string_target() {
        let mut scopes = Vec::new();
        let unmatched = verdict_condition(&value_test("x!~=\"a\""), positive(), |scope| {
            scopes.push(scope);
            ComplementTarget::Where(column("is_string"))
        });
        assert_eq!(scopes, [ComplementScope::StringTarget]);
        assert_statement_snapshot!(
            "verdict-condition-a-not-pattern-term-negates-the-pattern-over-a-string-target",
            render(unmatched)
        );
    }

    /// A complement of a positive condition no target meets holds for every
    /// target in its scope.
    #[test]
    fn the_complement_of_no_condition_is_every_target_in_scope() {
        let never = Condition::all().add(Expr::val(false));
        let unequal = verdict_condition(&value_test("x!=1"), never, |_| ComplementTarget::Where(column("present")));
        assert_statement_snapshot!("verdict-condition-the-complement-of-no-condition-is-every-target-in-scope", render(unequal));
    }

    /// Each array rule asks of a target holding several values what the
    /// reading derives: nothing, its presence, one passing element, or none.
    #[test]
    fn each_array_rule_reads_a_target_holding_several_values() {
        assert!(array_rule_condition(ArrayRule::Never, column("array"), || column("element")).is_none());
        for (index, rule) in [ArrayRule::Always, ArrayRule::SomeElement, ArrayRule::NoElement].into_iter().enumerate() {
            let condition = array_rule_condition(rule, column("array"), || column("element")).expect("a condition");
            assert_statement_snapshot!(
                format!("verdict-condition-each-array-rule-reads-a-target-holding-several-values-{}", index + 1),
                render(condition)
            );
        }
    }
}
