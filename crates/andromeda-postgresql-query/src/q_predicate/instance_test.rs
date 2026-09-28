//! A Query Term compiled as a test over the stored instances of one attribute,
//! before any probe asks whether an instance passes it.
//!
//! Most Query Terms compile to exactly that: an attribute's dictionary key and
//! a condition over one instance row. Keeping the two apart until the enclosing
//! expression is known lets a disjunction answer all such terms with one probe
//! instead of one probe each, which is what makes a disjunction reachable from
//! the value indexes at all: PostgreSQL turns a single `EXISTS` into a semi-join
//! it can drive from a bitmap over every term's index, while a disjunction of
//! separate `EXISTS` can only be evaluated as a filter over every entity row.

use crate::{attribute_name_key::AttributeNameKey, q_predicate::instance_scan::InstanceScan};
use sea_query::{Condition, SimpleExpr};

/// One Query Term as a condition over one instance of one attribute.
#[derive(Debug, Clone)]
pub(crate) struct InstanceTest {
    /// Dictionary key of the attribute whose instances the term reads.
    pub(crate) attribute: AttributeNameKey,
    /// What one instance of the attribute must satisfy.
    pub(crate) value: Condition,
}

impl InstanceTest {
    /// The test as one arm of a shared probe: the scan's own test for the
    /// attribute key, then the value condition.
    pub(crate) fn arm(self, of_attribute: SimpleExpr) -> Condition {
        Condition::all().add(of_attribute).add_option(non_empty(self.value))
    }
}

/// A condition that constrains anything, or `None` for the empty conjunction a
/// bare existence test carries, which would otherwise render as `TRUE`.
pub(crate) fn non_empty(condition: Condition) -> Option<Condition> {
    (!condition.is_empty()).then_some(condition)
}

/// A compiled Query Term, as far as its enclosing expression needs to know it.
#[derive(Debug, Clone)]
pub(crate) enum CompiledTerm {
    /// The term holds when one instance of one attribute passes the test.
    Instances(InstanceTest),
    /// The term is a condition of its own: a linked-entity hop, a payload
    /// reading, a disjunction of both readings of a trailing member, or a term
    /// no row can satisfy.
    Standalone(Condition),
}

impl CompiledTerm {
    /// The term as a condition on the entity the scan correlates with.
    pub(crate) fn into_condition(self, scan: &InstanceScan<'_>) -> Condition {
        match self {
            CompiledTerm::Instances(test) => scan.exists(test.attribute, test.value),
            CompiledTerm::Standalone(condition) => condition,
        }
    }
}

/// The terms of one disjunction, gathered so that every instance test shares
/// one probe.
///
/// Folding is exact: some instance passes one test or some instance passes
/// another exactly when some instance passes either, because every test reads
/// the same instances of the same entity. Clause 4.9 evaluates a Query Term
/// against "any Value of such instances", and clause 5.7.4.4 against the
/// instances the temporal window selected, so each test keeps its own attribute
/// key inside the shared probe and nothing else about the instances changes. A
/// conjunction cannot fold: two tests one instance must pass together are not
/// two tests that two instances pass separately.
#[derive(Debug, Default)]
pub(crate) struct Disjunction {
    /// Every instance test among the terms.
    tests: Vec<InstanceTest>,
    /// Every other term.
    standalone: Vec<Condition>,
}

impl Disjunction {
    /// Add one term to the disjunction.
    pub(crate) fn with(mut self, term: CompiledTerm) -> Disjunction {
        match term {
            CompiledTerm::Instances(test) => self.tests.push(test),
            CompiledTerm::Standalone(condition) => self.standalone.push(condition),
        }
        self
    }

    /// The disjunction as a condition on the entity the scan correlates with.
    pub(crate) fn into_condition(self, scan: &InstanceScan<'_>) -> Condition {
        let shared = scan.exists_any(self.tests);
        self.standalone.into_iter().chain(shared).fold(Condition::any(), Condition::add)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        q_predicate::{
            instance_scan::InstanceScan,
            instance_test::{CompiledTerm, Disjunction, InstanceTest},
        },
    };
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query};

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn test_of(attribute: i32, column: &str, value: i32) -> CompiledTerm {
        CompiledTerm::Instances(InstanceTest {
            attribute: AttributeNameKey::from(attribute),
            value: Condition::all().add(Expr::col(Alias::new(column)).eq(value)),
        })
    }

    #[test]
    fn every_instance_test_of_a_disjunction_shares_one_probe() {
        let rows = EntityRowRefs::unaliased(None);
        let scan = InstanceScan::Current(&rows);
        let sql = render(Disjunction::default().with(test_of(3, "a", 1)).with(test_of(4, "b", 2)).into_condition(&scan));
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(sql.contains(r#""cav"."attribute_name_key" = 3"#), "{sql}");
        assert!(sql.contains(r#""cav"."attribute_name_key" = 4"#), "{sql}");
    }

    #[test]
    fn a_standalone_term_stays_beside_the_shared_probe() {
        let rows = EntityRowRefs::unaliased(None);
        let scan = InstanceScan::Current(&rows);
        let standalone = CompiledTerm::Standalone(Condition::all().add(Expr::col(Alias::new("flag")).eq(true)));
        let sql = render(
            Disjunction::default()
                .with(test_of(3, "a", 1))
                .with(standalone)
                .with(test_of(4, "b", 2))
                .into_condition(&scan),
        );
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(sql.contains(r#""flag" = TRUE OR EXISTS"#), "{sql}");
    }

    #[test]
    fn a_lone_instance_test_keeps_the_single_attribute_probe() {
        let rows = EntityRowRefs::unaliased(None);
        let scan = InstanceScan::Current(&rows);
        let lone = render(Disjunction::default().with(test_of(3, "a", 1)).into_condition(&scan));
        let direct = render(test_of(3, "a", 1).into_condition(&scan));
        assert_eq!(lone, direct);
    }
}
