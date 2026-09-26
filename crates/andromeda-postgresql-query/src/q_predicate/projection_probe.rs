//! The correlated `EXISTS` probe over `current_attribute_values` that every
//! value predicate is wrapped in, and the alias it reads the scan under.
//!
//! One alias, one place: a predicate compiled here and a predicate compiled by
//! a linked-entity hop read the same relation under the same name, so a value
//! condition written against [`probe_column`] composes into either without
//! knowing which.

use crate::{attribute_name_key::AttributeNameKey, entity_row_refs::EntityRowRefs, instance_expiry::instance_stands};
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Iden, Query, SimpleExpr};

/// The alias every value probe reads the `current_attribute_values` scan under.
#[derive(Iden, Clone, Copy)]
#[iden = "cav"]
struct Cav;

/// A `"cav"."column"` reference into the probe's projection scan.
#[must_use]
pub fn probe_column(column: CurrentAttributeValues) -> SimpleExpr {
    SeaExpr::col((Cav, column))
}

/// The `EXISTS` probe that answers presence alone: a row for this attribute
/// under the enclosing statement's entity.
///
/// The probe reads the leading `(entity_key, attribute_name_key)` columns of the
/// clause 4.5.5 grain constraint, so it is an index-only lookup on the same
/// index the write path conflicts against.
#[must_use]
pub fn attribute_row_exists(id: AttributeNameKey, rows: &EntityRowRefs) -> Condition {
    exists_on_projection(id, Condition::all(), rows)
}

/// Wrap a value condition in the correlated `current_attribute_values` probe.
///
/// The probe is an `EXISTS` over the projection, narrowed to the attribute's
/// dictionary key and the value condition, and correlated on the enclosing
/// statement's entity key. The attribute-name test is added before the value so
/// its dictionary key binds ahead of the value's operands, and the correlation
/// is added last so it binds nothing after them. An instance whose clause 4.22
/// `expiresAt` has passed is not probed at all.
#[must_use]
pub fn exists_on_projection(id: AttributeNameKey, value: Condition, rows: &EntityRowRefs) -> Condition {
    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributeValues::Table).alias(Cav))
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::AttributeNameKey)).eq(id.get()))
        // Clause 4.22: a past-due instance neither carries the attribute nor
        // holds its value, whether or not the sweep has deleted it yet.
        .and_where(instance_stands(probe_column(CurrentAttributeValues::ExpiresAt)))
        .cond_where(value)
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::EntityKey)).eq(rows.entity_key_column()));
    Condition::all().add(SeaExpr::exists(probe))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        q_predicate::projection_probe::{attribute_row_exists, exists_on_projection, probe_column},
    };
    use andromeda_postgresql_schema::current::{CurrentAttributeValues, Entities};
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
        pretty_assertions::assert_eq!(
            render(attribute_row_exists(AttributeNameKey::from(7), &EntityRowRefs::unaliased(None))),
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_key" = $1 AND ("cav"."expires_at" IS NULL OR "cav"."expires_at" > NOW()) "#,
                r#"AND "cav"."entity_key" = "entities"."entity_key")"#,
            )
        );
    }

    /// The value condition sits between the attribute key and the correlation,
    /// so its operands bind after the dictionary key and before nothing.
    #[test]
    fn a_value_probe_places_its_condition_between_the_attribute_key_and_the_correlation() {
        let value = Condition::all().add(probe_column(CurrentAttributeValues::ValueString).eq("red"));
        let sql = render(exists_on_projection(AttributeNameKey::from(7), value, &EntityRowRefs::unaliased(None)));
        assert!(
            sql.contains(
                r#""cav"."attribute_name_key" = $1 AND ("cav"."expires_at" IS NULL OR "cav"."expires_at" > NOW()) AND "cav"."value_string" = $2 AND "cav"."entity_key""#
            ),
            "{sql}"
        );
    }
}
