//! Which stored attribute instances a `q` Query Term is checked against.
//!
//! A Query Entities `q` (clause 5.7.2) is a statement about the Entity as it
//! stands: its `current_attribute_values` rows and, for the shapes those cannot
//! answer, its payload. A temporal `q` (clause 5.7.4.4 step S2) is checked
//! against the Attribute instances the temporal window selected instead, which
//! are `attribute_history` rows. The comparisons themselves are the same clause
//! 4.9 comparisons in both cases -- the two relations store an instance's value
//! in the same typed columns -- so the compiler writes each comparison once and
//! this decides which rows it runs over.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    q_predicate::{
        history_instance_document::instance_document,
        history_window::{HistoryAliases, HistoryWindow},
        instance_test::InstanceTest,
        projection_probe::{exists_any_on_projection, exists_on_projection, projection_column},
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_schema::{current::CurrentAttributeValues, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::literal::false_literal;
use sea_query::{Condition, DynIden, SimpleExpr};

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

    /// A value column of the instance row the scan's probe reads.
    pub(crate) fn value(&self, column: ValueColumn) -> SimpleExpr {
        match self {
            InstanceScan::Current(_) => projection_column(column.in_projection()),
            InstanceScan::History(_) => HistoryAliases::top_level().instance_column(column.in_history()),
        }
    }

    /// The attribute family code of the instance row the scan's probe reads,
    /// which both relations record per instance.
    pub(crate) fn attribute_family(&self) -> SimpleExpr {
        match self {
            InstanceScan::Current(_) => projection_column(CurrentAttributeValues::AttributeKind),
            InstanceScan::History(_) => HistoryAliases::top_level().instance_column(AttributeHistory::AttributeKind),
        }
    }

    /// The columns a String target value can occupy, each named once.
    ///
    /// The projection moves a String past the B-tree bound into its overflow
    /// column; history has none and keeps every String in `value_string`.
    pub(crate) const fn string_columns(&self) -> &'static [ValueColumn] {
        match self {
            InstanceScan::Current(_) => &[ValueColumn::String, ValueColumn::LongText],
            InstanceScan::History(_) => &[ValueColumn::String],
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

    /// A condition over the expanded document of `attribute`, for the Query
    /// Terms only a jsonpath can answer.
    ///
    /// Over current state the document is the entity's payload and the
    /// condition stands on its own. Over history each in-window instance is
    /// rebuilt as the document of an entity holding that one instance, and the
    /// condition is checked inside the probe over those instances, so one
    /// instance that satisfies it is enough. An attribute the dictionary never
    /// recorded has no history, so it matches nothing.
    pub(crate) fn document_condition(&self, attribute: &AttributeName, names: &QueryNames, build: impl FnOnce(SimpleExpr) -> Condition) -> Condition {
        match self {
            InstanceScan::Current(rows) => build(rows.payload_column()),
            InstanceScan::History(window) => {
                let aliases = HistoryAliases::top_level();
                match names.attributes.get(attribute) {
                    Some(id) => window.exists(id, build(instance_document(attribute.as_str(), &aliases)), &aliases),
                    None => Condition::all().add(false_literal()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_row_refs::EntityRowRefs,
        q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, instance_test::InstanceTest, value_column::ValueColumn},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query};

    fn history_over(interval: TimeInterval) -> HistoryWindow {
        HistoryWindow::builder()
            .schema(None)
            .temporal_entity_key(Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))))
            .axis(AttributeHistory::ModifiedAt)
            .interval(interval)
            .build()
    }

    fn history() -> HistoryWindow {
        history_over(TimeInterval::Unbounded)
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_value_column_resolves_in_the_relation_the_scan_reads() {
        let rows = EntityRowRefs::unaliased(None);
        let window = history();
        let current = Query::select()
            .expr(InstanceScan::Current(&rows).value(ValueColumn::LongText))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        let past = Query::select()
            .expr(InstanceScan::History(&window).value(ValueColumn::LongText))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(current.contains(r#""cav"."value_long_text""#), "{current}");
        assert!(past.contains(r#""qh"."value_string""#), "{past}");
    }

    /// Both relations record the family of every instance, which is what
    /// decides whether a row's `value_object` is a `languageMap`.
    #[test]
    fn the_attribute_family_resolves_in_the_relation_the_scan_reads() {
        let rows = EntityRowRefs::unaliased(None);
        let window = history();
        let current = Query::select()
            .expr(InstanceScan::Current(&rows).attribute_family())
            .to_owned()
            .to_string(PostgresQueryBuilder);
        let past = Query::select()
            .expr(InstanceScan::History(&window).attribute_family())
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(current.contains(r#""cav"."attribute_kind""#), "{current}");
        assert!(past.contains(r#""qh"."attribute_kind""#), "{past}");
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
            (InstanceScan::Current(&rows), r#""cav"."attribute_name_key""#),
            (InstanceScan::History(&window), r#""qs"."attribute_name_key""#),
        ] {
            let sql = render(scan.exists_any(tests()).expect("two tests make a probe"));
            assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
            assert!(sql.contains(&format!("{key} = 3")) && sql.contains(&format!("{key} = 4")), "{sql}");
        }
    }

    /// A Query Term only a jsonpath answers is checked against every instance
    /// valid within the window, so the rebuilt instance document is read in
    /// the probe over the instances inside it and in the probe over the
    /// instance in effect when it opens (clause 4.11, Annex C clause C.5.5).
    #[test]
    fn a_document_condition_over_history_probes_both_window_and_in_effect_instances() {
        let address = || "https://example.org/address".parse::<AttributeName>().expect("a valid attribute name");
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(address(), AttributeNameKey::from(9))]),
            types: ResolvedNames::default(),
        };
        let start = DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        let window = history_over(TimeInterval::After(start));
        let sql =
            render(InstanceScan::History(&window).document_condition(&address(), &names, |document| Condition::all().add(Expr::expr(document).is_not_null())));
        assert_eq!(sql.matches("EXISTS").count(), 2, "{sql}");
        assert_eq!(
            sql.matches("jsonb_build_array(jsonb_build_object('https://example.org/address'").count(),
            2,
            "{sql}"
        );
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
    }

    #[test]
    fn no_test_makes_no_probe() {
        let rows = EntityRowRefs::unaliased(None);
        assert!(InstanceScan::Current(&rows).exists_any(Vec::new()).is_none());
    }
}
