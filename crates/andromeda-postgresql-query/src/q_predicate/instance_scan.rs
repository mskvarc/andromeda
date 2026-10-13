//! Which stored attribute instances a `q` Query Term is checked against.
//!
//! A Query Entities `q` (clause 5.7.2) is a statement about the Entity as it
//! stands: its `current_attributes` rows. A temporal `q` (clause 5.7.4.4 step
//! S2) is checked against the Attribute instances the temporal window selected
//! instead, which are `attribute_history` rows. The comparisons themselves are
//! the same clause 4.9 comparisons in both cases (the two relations store an
//! instance's value in the same typed columns), so the compiler writes each
//! comparison once and this decides which rows it runs over.

use crate::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        history_window::{HistoryAliases, HistoryWindow},
        instance_row::ProbedRow,
        instance_test::InstanceTest,
        projection_probe::{ProbeShape, exists_any_on_projection, exists_on_projection, probe},
    },
};
use sea_query::{Condition, DynIden};

/// The stored instances a compiled `q` reads.
#[derive(Debug, Clone, Copy)]
pub enum InstanceScan<'a> {
    /// The current state of the entity the enclosing statement names through
    /// these row references.
    Current(&'a EntityRowRefs),
    /// The instances of one Temporal Evolution inside a temporal window
    /// (clause 5.7.4.4).
    History(&'a HistoryWindow),
}

impl InstanceScan<'_> {
    /// The branch schema every relation the scan reads is qualified with.
    #[must_use]
    pub const fn schema(&self) -> Option<&DynIden> {
        match self {
            InstanceScan::Current(rows) => rows.schema(),
            InstanceScan::History(window) => window.schema(),
        }
    }

    /// The row each instance the scan reads is held in.
    #[must_use]
    pub const fn row(&self) -> ProbedRow {
        match self {
            InstanceScan::Current(_) => ProbedRow::Current,
            InstanceScan::History(_) => ProbedRow::History,
        }
    }

    /// Whether one instance of the attribute satisfies `value`, a recheck
    /// through its `sub_attributes` tree.
    ///
    /// Over current state the probe is walked in page order (see
    /// [`ProbeShape::Walked`]); a window's probes are already narrowed to one
    /// temporal entity's streams, so they stay as they are.
    pub(crate) fn exists_in_tree(&self, id: AttributeNameKey, value: Condition) -> Condition {
        match self {
            InstanceScan::Current(rows) => probe(id, value, rows, ProbeShape::Walked),
            InstanceScan::History(window) => window.exists(id, value, &HistoryAliases::top_level()),
        }
    }

    /// Whether one instance of the attribute satisfies `value`.
    pub(crate) fn exists(&self, id: AttributeNameKey, value: Condition) -> Condition {
        match self {
            InstanceScan::Current(rows) => exists_on_projection(id, value, rows),
            InstanceScan::History(window) => window.exists(id, value, &HistoryAliases::top_level()),
        }
    }

    /// Whether one instance passes any of `tests`, or `None` when there is no
    /// test to pass.
    ///
    /// A lone test is the single-attribute probe [`InstanceScan::exists`]
    /// renders; several share one probe whose instances each test reads under
    /// its own attribute key.
    pub(crate) fn exists_any(&self, tests: Vec<InstanceTest>) -> Option<Condition> {
        match <[InstanceTest; 1]>::try_from(tests) {
            Ok([test]) => Some(self.exists(test.attribute, test.value)),
            Err(tests) if tests.is_empty() => None,
            Err(tests) => Some(match self {
                InstanceScan::Current(rows) => exists_any_on_projection(tests, rows),
                InstanceScan::History(window) => window.exists_any(tests, &HistoryAliases::top_level()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, instance_test::InstanceTest},
    };
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use sea_query::{Alias, Condition, Expr, PostgresQueryBuilder, Query};

    fn history() -> HistoryWindow {
        HistoryWindow::builder()
            .schema(None)
            .temporal_entity_key(Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))))
            .axis(AttributeHistory::ModifiedAt)
            .interval(TimeInterval::Unbounded)
            .build()
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn presence_over_history_is_an_in_window_instance_of_the_attribute() {
        let window = history();
        let sql = render(InstanceScan::History(&window).exists(AttributeNameKey::from(3), Condition::all()));
        assert!(sql.contains(r#""qs"."attribute_name_key" = 3"#), "{sql}");
        assert!(sql.contains(r#""qh"."deleted_at" IS NULL"#), "{sql}");
    }

    /// Tests that share a probe keep their own attribute keys inside it, over
    /// the one set of instances the scan reads.
    #[test]
    fn several_tests_share_one_probe_over_either_scan() {
        let tests = || {
            vec![
                InstanceTest {
                    attribute: AttributeNameKey::from(3),
                    value: Condition::all(),
                },
                InstanceTest {
                    attribute: AttributeNameKey::from(4),
                    value: Condition::all(),
                },
            ]
        };
        let rows = EntityRowRefs::unaliased(None);
        let window = history();
        for (scan, key) in [
            (InstanceScan::Current(&rows), r#""ca"."attribute_name_key""#),
            (InstanceScan::History(&window), r#""qs"."attribute_name_key""#),
        ] {
            let sql = render(scan.exists_any(tests()).expect("two tests make a probe"));
            assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
            assert!(sql.contains(&format!("{key} = 3")) && sql.contains(&format!("{key} = 4")), "{sql}");
        }
    }

    #[test]
    fn no_test_makes_no_probe() {
        let rows = EntityRowRefs::unaliased(None);
        assert!(InstanceScan::Current(&rows).exists_any(Vec::new()).is_none());
    }
}
