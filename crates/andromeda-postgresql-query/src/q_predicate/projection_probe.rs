//! The correlated `EXISTS` probe over `current_attributes` that every value
//! predicate is wrapped in, and the alias it reads the instance rows under.
//!
//! One alias, one place: a predicate compiled here and a predicate compiled by
//! a linked-entity hop read the same relation under the same name, so a value
//! condition written against [`projection_column`] composes into either without
//! knowing which.

use crate::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    expiry::unexpired,
    q_predicate::instance_test::{InstanceTest, non_empty},
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr};

/// The alias every value probe reads the `current_attributes` rows under.
#[derive(Iden, Clone, Copy)]
#[iden = "ca"]
pub struct Ca;

/// A `"ca"."column"` reference into the probe's instance rows.
#[must_use]
pub fn projection_column(column: CurrentAttributes) -> SimpleExpr {
    SeaExpr::col((Ca, column))
}

/// How the planner may evaluate a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeShape {
    /// A plain `EXISTS`, which the planner may turn into a semi-join driven
    /// from whichever value index it prices cheapest.
    Planned,
    /// A probe evaluated once per Entity the statement walks, in the order it
    /// walks them.
    ///
    /// A predicate on a sub-attribute is underestimated by the planner (its
    /// recheck and its row filters are priced as independent), so a semi-join
    /// it picks for a broad predicate visits every match. An `EXISTS` carrying
    /// an `OFFSET` is never pulled up into a join, so it stays a per-row probe
    /// that stops at its first row, and a page cut in walk order stops after
    /// the Entities it keeps.
    Walked,
}

/// Wrap a value condition in the correlated `current_attributes` probe.
///
/// The probe is an `EXISTS` over the instance rows, narrowed to the
/// attribute's dictionary key and the value condition, and correlated on the
/// enclosing statement's entity key. The attribute-name test is added before
/// the value so its dictionary key binds ahead of the value's operands, and the
/// correlation is added last so it binds nothing after them. An instance whose
/// clause 4.22 `expiresAt` has passed is not probed at all.
///
/// With no value condition the probe answers presence alone, on the leading
/// `(entity_key, attribute_name_key)` columns of the clause 4.5.5 grain
/// constraint: an index-only lookup on the index the write path conflicts
/// against.
#[must_use]
pub fn exists_on_projection(id: AttributeNameKey, value: Condition, rows: &EntityRowRefs) -> Condition {
    probe(id, value, rows, ProbeShape::Planned)
}

/// [`exists_on_projection`] in the shape `shape` asks for.
pub(crate) fn probe(id: AttributeNameKey, value: Condition, rows: &EntityRowRefs, shape: ProbeShape) -> Condition {
    let mut probe = projection_probe(Condition::all().add(of_attribute(id)).add(standing()).add_option(non_empty(value)), rows);
    match shape {
        ProbeShape::Planned => {}
        ProbeShape::Walked => {
            probe.offset(0);
        }
    }
    Condition::all().add(SeaExpr::exists(probe))
}

/// The correlated probe that holds when one current instance passes any of
/// `tests`, each read under its own attribute key.
///
/// The attribute key moves into each test rather than narrowing the scan, so
/// every test is a conjunction the value index of its own column leads with,
/// and the planner can drive the whole probe from a bitmap over all of them.
#[must_use]
pub(crate) fn exists_any_on_projection(tests: Vec<InstanceTest>, rows: &EntityRowRefs) -> Condition {
    let any = tests.into_iter().fold(Condition::any(), |any, test| {
        let of_attribute = of_attribute(test.attribute);
        any.add(test.arm(of_attribute))
    });
    Condition::all().add(SeaExpr::exists(projection_probe(Condition::all().add(standing()).add(any), rows)))
}

/// An instance of the attribute with this dictionary key.
#[must_use]
pub fn of_attribute(id: AttributeNameKey) -> SimpleExpr {
    SeaExpr::expr(projection_column(CurrentAttributes::AttributeNameKey)).eq(id.get())
}

/// Clause 4.22: a past-due instance neither carries the attribute nor holds its
/// value, whether or not the sweep has deleted it yet.
#[must_use]
pub fn standing() -> SimpleExpr {
    unexpired(projection_column(CurrentAttributes::ExpiresAt))
}

/// The select over the instance rows that pass `filter`, correlated last on
/// the enclosing statement's entity key.
fn projection_probe(filter: Condition, rows: &EntityRowRefs) -> SelectStatement {
    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .cond_where(filter)
        .and_where(SeaExpr::expr(projection_column(CurrentAttributes::EntityKey)).eq(rows.entity_key_column()));
    probe.take()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        q_predicate::{
            instance_test::InstanceTest,
            projection_probe::{ProbeShape, exists_any_on_projection, exists_on_projection, probe, projection_column},
        },
    };
    use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
    use pretty_assertions::assert_eq;
    use sea_query::{Condition, ExprTrait, PostgresQueryBuilder, Query};

    fn render(condition: Condition) -> String {
        let (sql, _) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        sql
    }

    /// A presence test carries no value condition at all, so the probe stays on
    /// the grain constraint's leading columns.
    #[test]
    fn a_presence_probe_tests_the_attribute_key_and_the_correlation_and_nothing_else() {
        assert_eq!(
            render(exists_on_projection(
                AttributeNameKey::from(7),
                Condition::all(),
                &EntityRowRefs::unaliased(None)
            )),
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attributes" AS "ca" "#,
                r#"WHERE "ca"."attribute_name_key" = $1 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) "#,
                r#"AND "ca"."entity_key" = "entities"."entity_key")"#,
            )
        );
    }

    /// The value condition sits between the attribute key and the correlation,
    /// so its operands bind after the dictionary key and before nothing.
    #[test]
    fn a_value_probe_places_its_condition_between_the_attribute_key_and_the_correlation() {
        let value = Condition::all().add(projection_column(CurrentAttributes::ValueString).eq("red"));
        let sql = render(exists_on_projection(AttributeNameKey::from(7), value, &EntityRowRefs::unaliased(None)));
        assert!(
            sql.contains(
                r#""ca"."attribute_name_key" = $1 AND ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) AND "ca"."value_string" = $2 AND "ca"."entity_key""#
            ),
            "{sql}"
        );
    }

    /// A walked probe carries an `OFFSET`, which keeps PostgreSQL from pulling
    /// it up into a semi-join.
    #[test]
    fn a_walked_probe_carries_an_offset() {
        let (sql, values) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(probe(
                AttributeNameKey::from(7),
                Condition::all(),
                &EntityRowRefs::unaliased(None),
                ProbeShape::Walked,
            ))
            .build(PostgresQueryBuilder);
        assert!(sql.contains(r#""ca"."entity_key" = "entities"."entity_key" OFFSET $2)"#), "{sql}");
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    /// A shared probe tests expiry once and then each attribute key beside its
    /// own value, so every arm leads with the columns of its value index, and
    /// the arms bind in order before the correlation.
    #[test]
    fn a_shared_probe_pairs_each_attribute_key_with_its_own_value() {
        let test = |attribute: i32, value: &str| InstanceTest {
            attribute: AttributeNameKey::from(attribute),
            value: Condition::all().add(projection_column(CurrentAttributes::ValueUri).eq(value)),
        };
        assert_eq!(
            render(exists_any_on_projection(
                vec![test(7, "urn:a"), test(8, "urn:b")],
                &EntityRowRefs::unaliased(None)
            )),
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attributes" AS "ca" "#,
                r#"WHERE ("ca"."expires_at" IS NULL OR "ca"."expires_at" > NOW()) "#,
                r#"AND (("ca"."attribute_name_key" = $1 AND "ca"."value_uri" = $2) OR ("ca"."attribute_name_key" = $3 AND "ca"."value_uri" = $4)) "#,
                r#"AND "ca"."entity_key" = "entities"."entity_key")"#,
            )
        );
    }
}
