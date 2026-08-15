//! Correlating a subquery to one entity's geometry-valued attribute instances.
//!
//! Clause 4.10 lets a geo-query name any GeoProperty of an entity, and clause
//! 4.23 lets a distance ordering measure to one. Every GeoProperty other than
//! the default `location` is stored as `current_attribute_values` rows — one per
//! `datasetId` instance — rather than as a column of `entities`, so both the
//! filter and the ordering reach it through the same correlated subquery. That
//! fragment is written once here: a predicate and an ordering that disagreed
//! about which rows a GeoProperty lives in would silently order by a geometry
//! they did not filter on.

use crate::attribute_name_id::AttributeNameId;
use andromeda_postgresql_schema::current::{CurrentAttributeValues, Entities};
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Alias, Expr as SeaExpr, ExprTrait, IntoIden, Query, SelectStatement, SimpleExpr};

/// The `entities` row a correlated subquery binds back to.
///
/// The binding is carried as a value rather than assumed, because the enclosing
/// statement chooses whether `entities` is joined under an alias: the entity
/// path leaves it bare, the temporal path joins it as `e`. A correlated
/// subquery that named the identifier column unqualified would rebind itself to
/// whichever relation a later join happened to make visible, so the outer row is
/// always stated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrelatedEntityRow<'a> {
    /// Table alias the statement joins `entities` under, or `None` when the
    /// table is used unaliased.
    alias: Option<&'a str>,
}

impl<'a> CorrelatedEntityRow<'a> {
    /// The row of an `entities` table used without an alias.
    pub const fn unaliased() -> CorrelatedEntityRow<'a> {
        CorrelatedEntityRow { alias: None }
    }

    /// The row of an `entities` table joined under `alias`.
    pub const fn aliased(alias: &'a str) -> CorrelatedEntityRow<'a> {
        CorrelatedEntityRow { alias: Some(alias) }
    }

    /// One of the row's columns, always qualified by the name the enclosing
    /// statement knows the row under: an aliased `e` and `location` give
    /// `"e"."location"`, an unaliased row gives `"entities"."location"`.
    ///
    /// The qualification is what makes the correlation a correlation. A
    /// subquery over `current_attribute_values` has an `entity_key` column of
    /// its own, and SQL resolves an unqualified name in the innermost scope
    /// that offers one, so `"cav"."entity_key" = "entity_key"` is not a
    /// reference to the outer row at all — it compares the subquery's column
    /// with itself and silently admits every instance in the tenant.
    pub fn column(&self, column: impl IntoIden) -> SimpleExpr {
        match self.alias {
            Some(alias) => SeaExpr::col((Alias::new(alias), column.into_iden())),
            None => SeaExpr::col((Entities::Table, column.into_iden())),
        }
    }

    /// The row's `entity_key`, the surrogate a correlated subquery binds against.
    pub fn entity_key_column(&self) -> SimpleExpr {
        self.column(Entities::EntityKey)
    }
}

/// A `current_attribute_values` scan selecting one entity's geometry-valued
/// instances of one attribute, correlated to the outer row and left open so a
/// caller can add its projection and any further condition.
///
/// `subquery_alias` names the `current_attribute_values` occurrence inside the
/// subquery; each caller passes its own so two correlated subqueries nested in
/// one statement cannot capture each other's rows.
///
/// Both of the equality columns and the `value_geo IS NOT NULL` restriction are
/// covered by the partial btree index `idx_cav_attr_entity_geo
/// (attribute_name_id, entity_key)`, so the scan is an index probe rather than a
/// scan of every instance of the attribute.
///
/// The correlation reads the outer row's `entity_key`: the projection is owned
/// by the `entities` row through the surrogate key, and the entity URI it used
/// to carry is not a column of this relation at all.
pub fn geometry_instances_of(outer: CorrelatedEntityRow<'_>, subquery_alias: &str, attribute_id: AttributeNameId) -> SelectStatement {
    let alias = Alias::new(subquery_alias);
    let mut scan = Query::select();
    scan.from_as(CurrentAttributeValues::Table, alias.clone())
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributeValues::EntityKey)).eq(outer.entity_key_column()))
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributeValues::AttributeNameId)).eq(attribute_id.get()))
        .and_where(SeaExpr::col((alias, CurrentAttributeValues::ValueGeo)).is_not_null());
    scan
}

/// The subquery's geometry column: the `value_geo` of the instance rows
/// [`geometry_instances_of`] selects.
pub fn instance_geometry_column(subquery_alias: &str) -> SimpleExpr {
    SeaExpr::col((Alias::new(subquery_alias), CurrentAttributeValues::ValueGeo))
}

/// A `SELECT 1` marker for a correlated `EXISTS` built over
/// [`geometry_instances_of`], kept beside the scan so both crates that wrap the
/// scan in an existential spell the marker the same way.
pub fn existence_marker() -> SimpleExpr {
    int_literal(1)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instance_correlation::{CorrelatedEntityRow, existence_marker, geometry_instances_of, instance_geometry_column},
        attribute_name_id::AttributeNameId,
    };
    use sea_query::{PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

    /// Render one correlated scan to its SQL, giving it the `SELECT 1` marker
    /// each real caller supplies.
    fn render(mut scan: SelectStatement) -> String {
        scan.expr(existence_marker());
        scan.to_string(PostgresQueryBuilder)
    }

    /// Render one column expression to its SQL alone.
    fn render_column(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Every column reference carries a qualifier: the statement's alias where
    /// there is one, and the table's own name where there is not.
    #[test]
    fn a_column_is_qualified_by_the_alias_or_by_the_table_name() {
        assert!(render_column(CorrelatedEntityRow::aliased("e").column(sea_query::Alias::new("location_geo"))).contains("\"e\".\"location_geo\""));
        assert!(render_column(CorrelatedEntityRow::unaliased().column(sea_query::Alias::new("location_geo"))).contains("\"entities\".\"location_geo\""));
    }

    /// The correlation names the outer relation explicitly, so a join added to
    /// the enclosing statement cannot silently rebind it — and, more sharply,
    /// so the subquery's own `entity_key` cannot capture the comparison and turn
    /// it into a tautology over every instance in the tenant.
    #[test]
    fn the_correlation_binds_the_outer_row_it_was_given() {
        let bare = render(geometry_instances_of(
            CorrelatedEntityRow::unaliased(),
            "geo_instance",
            AttributeNameId::from(42_i32),
        ));
        assert!(bare.contains("\"geo_instance\".\"entity_key\" = \"entities\".\"entity_key\""), "{bare}");

        let aliased = render(geometry_instances_of(
            CorrelatedEntityRow::aliased("e"),
            "geo_instance",
            AttributeNameId::from(42_i32),
        ));
        assert!(aliased.contains("\"geo_instance\".\"entity_key\" = \"e\".\"entity_key\""), "{aliased}");
    }

    /// No branch may leave the outer reference unqualified, whatever column it
    /// names: an unqualified reference is the exact shape that silently stops
    /// correlating.
    #[test]
    fn no_correlation_leaves_the_outer_reference_unqualified() {
        for row in [CorrelatedEntityRow::unaliased(), CorrelatedEntityRow::aliased("e")] {
            let sql = render(geometry_instances_of(row, "geo_instance", AttributeNameId::from(42_i32)));
            assert!(!sql.contains("= \"entity_key\""), "{sql}");
        }
    }

    /// The projection is owned by the `entities` row through the surrogate key,
    /// so the entity URI appears nowhere in the correlation.
    #[test]
    fn the_correlation_reads_the_surrogate_key_and_never_the_entity_uri() {
        let sql = render(geometry_instances_of(
            CorrelatedEntityRow::aliased("e"),
            "geo_instance",
            AttributeNameId::from(42_i32),
        ));
        assert!(!sql.contains("entity_id"), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    /// Clause 4.10 reads the instances of one named GeoProperty, so the scan
    /// restricts on the dictionary key and on the geometry column being
    /// populated — the two halves the supporting partial index is built on.
    #[test]
    fn the_correlation_restricts_to_the_geometry_instances_of_one_attribute() {
        let sql = render(geometry_instances_of(
            CorrelatedEntityRow::unaliased(),
            "geo_order",
            AttributeNameId::from(7_i32),
        ));
        assert!(sql.contains("FROM \"current_attribute_values\" AS \"geo_order\""), "{sql}");
        assert!(sql.contains("\"geo_order\".\"attribute_name_id\" = 7"), "{sql}");
        assert!(sql.contains("\"geo_order\".\"value_geo\" IS NOT NULL"), "{sql}");
    }

    /// Two correlated subqueries in one statement read their own alias, so
    /// neither can capture the other's rows.
    #[test]
    fn each_caller_names_its_own_subquery_alias() {
        let first = render(geometry_instances_of(
            CorrelatedEntityRow::unaliased(),
            "geo_instance",
            AttributeNameId::from(1_i32),
        ));
        let second = render(geometry_instances_of(
            CorrelatedEntityRow::unaliased(),
            "geo_order",
            AttributeNameId::from(1_i32),
        ));
        assert!(!first.contains("geo_order"), "{first}");
        assert!(!second.contains("geo_instance"), "{second}");
        assert!(render_column(instance_geometry_column("geo_order")).contains("\"geo_order\".\"value_geo\""));
    }
}
