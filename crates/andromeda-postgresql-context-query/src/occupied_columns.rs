//! Which value columns the instances of one Attribute occupy, read once per
//! statement.
//!
//! An instance sets exactly one value column, so an Attribute none of whose
//! instances sets any column but one holds its values in that column alone.
//! Each column has a partial index leading on the Attribute's key, and each
//! probe asks it for one instance. A plain `EXISTS` is planned for its first
//! row, which the planner prices from estimates it takes as independent: when
//! other Attributes fill most of a column, a sequential scan looks like the
//! quickest way to a first row, finds none of this Attribute's, and reads the
//! whole relation. The probes therefore take two forms:
//!
//! - a column a B-tree orders the Attribute's instances by is probed with an
//!   ordered scalar subquery, `(SELECT 1 ... ORDER BY ... LIMIT 1) IS NOT
//!   NULL`. PostgreSQL drops the `ORDER BY` of an `EXISTS` but keeps that of a
//!   scalar subquery, and only the index yields the order, so the probe reads
//!   the index whatever the estimate. A geometry is read in entity order, which
//!   its attribute-led B-tree on the entity key yields;
//! - the three columns no B-tree orders (an object, a long text, a long URI)
//!   are probed with an `EXISTS` that also names the value kind the row pins
//!   to the column. The statistics on the Attribute's key and the value kind
//!   keep their most common pairs, so the planner sees how few of the
//!   Attribute's instances hold that kind.
//!
//! Every probe reads every instance, standing or not: an expired instance of
//! another kind closes a walk, which keeps the probes on their indexes. The
//! probes run once, in a materialised CTE every gate of the statement reads.

use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    q_predicate::projection_probe::{Ca, of_attribute, projection_column},
};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Condition, Expr, ExprTrait, Iden, Order, Query, SelectStatement, SimpleExpr};

/// The CTE holding, for each value column, whether an instance of the
/// Attribute sets it.
#[derive(Iden, Clone, Copy)]
#[iden = "occupied_columns"]
pub(crate) struct OccupiedColumns;

/// One value column of a `current_attributes` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeldColumn {
    /// `value_number`.
    Number,
    /// `value_string`: a String, a `Date` or a `Time` short enough to key a
    /// B-tree.
    String,
    /// `value_long_text`: a String too long to key a B-tree.
    LongText,
    /// `value_boolean`.
    Boolean,
    /// `value_geometry`.
    Geometry,
    /// `value_object`: an object or an array.
    Object,
    /// `value_date_time`: a `DateTime`, held as its instant.
    DateTime,
    /// `value_uri`: a Relationship's object short enough to key a B-tree.
    Uri,
    /// `value_long_uri`: a Relationship's object too long to key a B-tree.
    LongUri,
}

/// How the presence of an Attribute's instance in a column is probed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    /// The first instance in the order of this column, which a B-tree led by
    /// the Attribute's key yields.
    Ordered(CurrentAttributes),
    /// Any instance of this value kind, the one the row pins to the column.
    Kinded(ValueKind),
}

impl HeldColumn {
    /// Every value column, in the order the probes are projected.
    pub(crate) const ALL: [HeldColumn; 9] = [
        HeldColumn::Number,
        HeldColumn::String,
        HeldColumn::LongText,
        HeldColumn::Boolean,
        HeldColumn::Geometry,
        HeldColumn::Object,
        HeldColumn::DateTime,
        HeldColumn::Uri,
        HeldColumn::LongUri,
    ];

    /// The column.
    pub(crate) const fn column(self) -> CurrentAttributes {
        match self {
            HeldColumn::Number => CurrentAttributes::ValueNumber,
            HeldColumn::String => CurrentAttributes::ValueString,
            HeldColumn::LongText => CurrentAttributes::ValueLongText,
            HeldColumn::Boolean => CurrentAttributes::ValueBoolean,
            HeldColumn::Geometry => CurrentAttributes::ValueGeometry,
            HeldColumn::Object => CurrentAttributes::ValueObject,
            HeldColumn::DateTime => CurrentAttributes::ValueDateTime,
            HeldColumn::Uri => CurrentAttributes::ValueUri,
            HeldColumn::LongUri => CurrentAttributes::ValueLongUri,
        }
    }

    /// How the column is probed. A kind named here is exact: the row
    /// constrains the kind to agree with the column it sets.
    pub(crate) const fn probe(self) -> Probe {
        match self {
            HeldColumn::Number => Probe::Ordered(CurrentAttributes::ValueNumber),
            HeldColumn::String => Probe::Ordered(CurrentAttributes::ValueString),
            HeldColumn::Boolean => Probe::Ordered(CurrentAttributes::ValueBoolean),
            HeldColumn::DateTime => Probe::Ordered(CurrentAttributes::ValueDateTime),
            HeldColumn::Uri => Probe::Ordered(CurrentAttributes::ValueUri),
            HeldColumn::Geometry => Probe::Ordered(CurrentAttributes::EntityKey),
            HeldColumn::Object => Probe::Kinded(ValueKind::Object),
            HeldColumn::LongText => Probe::Kinded(ValueKind::String),
            HeldColumn::LongUri => Probe::Kinded(ValueKind::Uri),
        }
    }
}

/// One row with one boolean per value column, named as the column: whether an
/// instance of `attribute` sets it.
pub(crate) fn occupied_columns(rows: &EntityRowRefs, attribute: AttributeNameKey) -> SelectStatement {
    let mut occupied = Query::select();
    for column in HeldColumn::ALL {
        occupied.expr_as(occupied_probe(rows, attribute, column), column.column());
    }
    occupied.take()
}

/// Whether no instance of the Attribute sets a column other than `column`,
/// read from [`OccupiedColumns`].
///
/// An uncorrelated scalar subquery, which PostgreSQL evaluates once and turns
/// into a one-time filter, and which beside other filters stays one term.
pub(crate) fn held_alone(column: HeldColumn) -> SimpleExpr {
    let others = HeldColumn::ALL
        .into_iter()
        .filter(|other| *other != column)
        .fold(Condition::any(), |others, other| others.add(Expr::col(other.column())));
    Query::select().expr(others.not()).from(OccupiedColumns).take().into()
}

/// Whether an instance of `attribute`, standing or not, sets `column`.
fn occupied_probe(rows: &EntityRowRefs, attribute: AttributeNameKey, column: HeldColumn) -> SimpleExpr {
    let mut instance = Query::select();
    instance
        .expr(int_literal(1))
        .from(rows.table(CurrentAttributes::Table).alias(Ca))
        .and_where(of_attribute(attribute));
    match column.probe() {
        Probe::Ordered(order) => {
            instance
                .and_where(projection_column(column.column()).is_not_null())
                .order_by_expr(projection_column(order), Order::Asc)
                .limit(1);
            SimpleExpr::from(instance.take()).is_not_null()
        }
        Probe::Kinded(kind) => {
            instance
                .and_where(projection_column(CurrentAttributes::ValueKind).eq(int_literal(i64::from(kind.code()))))
                .and_where(projection_column(column.column()).is_not_null());
            Expr::exists(instance.take())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::occupied_columns::{HeldColumn, Probe, held_alone, occupied_columns};
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_row_refs::EntityRowRefs};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    const COLUMNS: [&str; 9] = [
        "value_number",
        "value_string",
        "value_long_text",
        "value_boolean",
        "value_geometry",
        "value_object",
        "value_date_time",
        "value_uri",
        "value_long_uri",
    ];

    fn rendered() -> String {
        occupied_columns(&EntityRowRefs::unaliased(None), AttributeNameKey::from(7)).to_string(PostgresQueryBuilder)
    }

    fn rendered_held_alone(column: HeldColumn) -> String {
        Query::select().expr(held_alone(column)).take().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn every_value_column_is_probed_once() {
        let sql = rendered();
        assert_eq!(sql.matches(r#"FROM "current_attributes" AS "ca""#).count(), 9, "{sql}");
        for column in COLUMNS {
            assert_eq!(sql.matches(&format!(r#"AS "{column}""#)).count(), 1, "{column}: {sql}");
        }
    }

    #[test]
    fn a_column_a_b_tree_orders_is_probed_in_that_order() {
        let sql = rendered();
        for column in ["value_number", "value_string", "value_boolean", "value_date_time", "value_uri"] {
            assert!(
                sql.contains(&format!(
                    r#"(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 AND "ca"."{column}" IS NOT NULL ORDER BY "ca"."{column}" ASC LIMIT 1) IS NOT NULL AS "{column}""#
                )),
                "{column}: {sql}"
            );
        }
    }

    #[test]
    fn a_geometry_is_probed_in_entity_order() {
        let sql = rendered();
        assert!(
            sql.contains(r#"AND "ca"."value_geometry" IS NOT NULL ORDER BY "ca"."entity_key" ASC LIMIT 1) IS NOT NULL AS "value_geometry""#),
            "{sql}"
        );
    }

    #[test]
    fn a_column_no_b_tree_orders_is_probed_with_the_kind_it_holds() {
        let sql = rendered();
        for (kind, column) in [(5, "value_object"), (2, "value_long_text"), (7, "value_long_uri")] {
            assert!(
                sql.contains(&format!(
                    r#"EXISTS(SELECT 1 FROM "current_attributes" AS "ca" WHERE "ca"."attribute_name_key" = 7 AND "ca"."value_kind" = ({kind}) AND "ca"."{column}" IS NOT NULL) AS "{column}""#
                )),
                "{column}: {sql}"
            );
        }
        assert_eq!(sql.matches(r#""ca"."value_kind""#).count(), 3, "{sql}");
    }

    /// The row's kind constraint pins an object to kind 5, a long text to the
    /// String kind 2 and a long URI to the URI kind 7.
    #[test]
    fn the_kind_a_probe_names_is_the_one_the_row_pins_to_its_column() {
        for (column, kind, code) in [
            (HeldColumn::Object, ValueKind::Object, 5),
            (HeldColumn::LongText, ValueKind::String, 2),
            (HeldColumn::LongUri, ValueKind::Uri, 7),
        ] {
            assert_eq!(column.probe(), Probe::Kinded(kind));
            assert_eq!(kind.code(), code);
        }
    }

    #[test]
    fn a_probe_reads_every_instance_standing_or_not() {
        let sql = rendered();
        assert!(!sql.contains("expires_at"), "{sql}");
    }

    #[test]
    fn a_column_held_alone_reads_every_other_column() {
        assert_eq!(
            rendered_held_alone(HeldColumn::DateTime),
            concat!(
                r#"SELECT (SELECT NOT ("value_number" OR "value_string" OR "value_long_text" OR "value_boolean" OR "value_geometry" OR "value_object" "#,
                r#"OR "value_uri" OR "value_long_uri") FROM "occupied_columns")"#
            )
        );
        let number = rendered_held_alone(HeldColumn::Number);
        assert!(number.contains(r#""value_date_time""#), "{number}");
        assert!(!number.contains(r#""value_number""#), "{number}");
    }
}
