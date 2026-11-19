//! What running a Query Term costs in this execution, the order a prepared
//! filter runs the terms of a group in.

use andromeda_ngsi_ld_query::filter::reading::{
    scalar_test::ScalarTest,
    term_reading::{TermReading, Test},
    terminal::Terminal,
};

/// How much walking and comparing a term costs, cheapest first.
///
/// The order is a fact of this execution: an existence term stops at the first
/// instance, a scalar comparison reads one value, a pattern runs a regular
/// expression over a string, a compound member walk descends into a value and
/// a hop reads another entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CostClass {
    /// The attribute, or what its path ends on, exists.
    Existence,
    /// A value, a value list, an ordering or a range, compared with a target.
    Scalar,
    /// A pattern matched against a string target.
    Pattern,
    /// A member of a compound value read before the test.
    CompoundMember,
    /// A hop into a linked entity.
    Hop,
}

impl CostClass {
    /// The class of `term`: the costliest step its path takes, else the cost
    /// of its test.
    #[must_use]
    pub const fn of(term: &TermReading) -> CostClass {
        if !term.hops().is_empty() {
            return CostClass::Hop;
        }
        match term.terminal() {
            Terminal::Members(_) | Terminal::ByFamily { .. } => CostClass::CompoundMember,
            Terminal::OwnValue | Terminal::InstanceMember(_) | Terminal::Language { .. } => match term.test() {
                Test::Exists => CostClass::Existence,
                Test::Value(test) => match test.test() {
                    ScalarTest::Matches(_) => CostClass::Pattern,
                    ScalarTest::EqualsAny(_) | ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Unsatisfiable => CostClass::Scalar,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q::cost_class::CostClass;
    use andromeda_ngsi_ld_query::{
        filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading},
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
    };

    /// The class of the one term `q` is.
    fn class_of(q: &str) -> CostClass {
        let reading = FilterReading::of(
            &EXAMPLE_NAMESPACE
                .expanded(q.parse::<Filter<Written>>().expect("a clause 4.9 filter"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        );
        let terms = reading.terms();
        assert_eq!(terms.len(), 1, "{q}");
        CostClass::of(terms[0])
    }

    #[test]
    fn each_kind_of_term_has_its_class() {
        assert_eq!(class_of("a"), CostClass::Existence);
        assert_eq!(class_of("a==1"), CostClass::Scalar);
        assert_eq!(class_of("a!=\"x\""), CostClass::Scalar);
        assert_eq!(class_of("a>1"), CostClass::Scalar);
        assert_eq!(class_of("a==1..5"), CostClass::Scalar);
        assert_eq!(class_of("a~=\"x\""), CostClass::Pattern);
        assert_eq!(class_of("a[city_name]==1"), CostClass::CompoundMember);
        assert_eq!(class_of("a[city_name]"), CostClass::CompoundMember);
        assert_eq!(class_of("r{a}==1"), CostClass::Hop);
    }

    #[test]
    fn the_classes_run_cheapest_first() {
        let mut classes = [
            CostClass::Hop,
            CostClass::Pattern,
            CostClass::Existence,
            CostClass::CompoundMember,
            CostClass::Scalar,
        ];
        classes.sort();
        assert_eq!(
            classes,
            [
                CostClass::Existence,
                CostClass::Scalar,
                CostClass::Pattern,
                CostClass::CompoundMember,
                CostClass::Hop
            ]
        );
    }
}
