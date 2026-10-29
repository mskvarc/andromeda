//! Correlating a subquery to one entity's geometry-valued attribute instances.
//!
//! Clause 4.10 lets a geo-query name any `GeoProperty` of an entity, and clause
//! 4.23 lets a distance ordering measure to one. Every `GeoProperty` other than
//! the default `location` is stored as `current_attributes` rows (one per
//! `datasetId` instance) rather than as a column of `entity_search`, so both the
//! filter and the ordering reach it through the same correlated subquery. That
//! fragment is written once here: a predicate and an ordering that disagreed
//! about which rows a `GeoProperty` lives in would silently order by a geometry
//! they did not filter on.

use crate::{attribute_name_key::AttributeNameKey, entity_row_refs::EntityRowRefs, instance_expiry::instance_stands, stored_geometry::StoredGeometry};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{literal::int_literal, postgis::judged_geometry};
use sea_query::{Alias, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// A `current_attributes` scan selecting one entity's geometry-valued
/// instances of one attribute, correlated to the outer row and left open so a
/// caller can add its projection and any further condition.
///
/// `subquery_alias` names the `current_attributes` occurrence inside the
/// subquery; each caller passes its own so two correlated subqueries nested in
/// one statement cannot capture each other's rows.
///
/// Both equality columns and the `value_geometry IS NOT NULL` restriction are
/// covered by the partial btree index `idx_ca_entity_geometry
/// (attribute_name_key, entity_key)`, so the scan is an index probe rather than a
/// scan of every instance of the attribute.
///
/// The correlation reads the outer row's `entity_key`: the rows are owned by
/// the `entities` row through the surrogate key, and the entity URI is not a
/// column of this relation at all.
#[must_use]
pub fn geometry_instances_of(outer: &EntityRowRefs, subquery_alias: &str, attribute_name_key: AttributeNameKey) -> SelectStatement {
    let alias = Alias::new(subquery_alias);
    let mut scan = Query::select();
    scan.from(outer.table(CurrentAttributes::Table).alias(alias.clone()))
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributes::EntityKey)).eq(outer.entity_key_column()))
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributes::AttributeNameKey)).eq(attribute_name_key.get()))
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributes::ValueGeometry)).is_not_null())
        // Clause 4.22: a past-due instance neither matches nor is measured to.
        .and_where(instance_stands(SeaExpr::col((alias, CurrentAttributes::ExpiresAt))));
    scan
}

/// The geometry a spatial predicate or a distance judges of the instance rows
/// [`geometry_instances_of`] selects: `COALESCE(value_geometry_valid,
/// value_geometry)`, the repaired form of an invalid written geometry and the
/// written one otherwise, spelled as `idx_ca_value_geometry` indexes it.
#[must_use]
pub fn instance_geometry_column(subquery_alias: &str) -> SimpleExpr {
    let alias = Alias::new(subquery_alias);
    judged_geometry(
        SeaExpr::col((alias.clone(), CurrentAttributes::ValueGeometryValid)), // clone: the alias qualifies both columns
        SeaExpr::col((alias, CurrentAttributes::ValueGeometry)),
    )
}

/// The instance rows [`geometry_instances_of`] selects as a geo-query reads
/// them: the geometry [`instance_geometry_column`] judges, and the box
/// `idx_ca_value_geometry` keys, its great-circle reach first.
#[must_use]
pub fn instance_stored_geometry(subquery_alias: &str) -> StoredGeometry {
    let alias = Alias::new(subquery_alias);
    StoredGeometry::written(
        SeaExpr::col((alias.clone(), CurrentAttributes::ValueGeometryReach)), // clone: the alias qualifies every column
        SeaExpr::col((alias.clone(), CurrentAttributes::ValueGeometryValid)),
        SeaExpr::col((alias, CurrentAttributes::ValueGeometry)),
    )
}

/// A `SELECT 1` marker for a correlated `EXISTS` built over
/// [`geometry_instances_of`], kept beside the scan so both crates that wrap the
/// scan in an existential spell the marker the same way.
#[must_use]
pub fn existence_marker() -> SimpleExpr {
    int_literal(1)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instance_correlation::{existence_marker, geometry_instances_of, instance_geometry_column},
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
    };
    use sea_query::{Alias, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};

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

    fn aliased() -> EntityRowRefs {
        EntityRowRefs::aliased(None, Alias::new("e"), Alias::new("s"))
    }

    /// The correlation names the outer relation explicitly, so a join added to
    /// the enclosing statement cannot silently rebind it, and, more sharply,
    /// so the subquery's own `entity_key` cannot capture the comparison and turn
    /// it into a tautology over every instance in the tenant.
    #[test]
    fn the_correlation_binds_the_outer_row_it_was_given() {
        let bare = render(geometry_instances_of(
            &EntityRowRefs::unaliased(None),
            "geo_instance",
            AttributeNameKey::from(42_i32),
        ));
        assert!(bare.contains("\"geo_instance\".\"entity_key\" = \"entities\".\"entity_key\""), "{bare}");

        let aliased = render(geometry_instances_of(&aliased(), "geo_instance", AttributeNameKey::from(42_i32)));
        assert!(aliased.contains("\"geo_instance\".\"entity_key\" = \"e\".\"entity_key\""), "{aliased}");
    }

    /// No branch may leave the outer reference unqualified, whatever column it
    /// names: an unqualified reference is the exact shape that silently stops
    /// correlating.
    #[test]
    fn no_correlation_leaves_the_outer_reference_unqualified() {
        for rows in [EntityRowRefs::unaliased(None), aliased()] {
            let sql = render(geometry_instances_of(&rows, "geo_instance", AttributeNameKey::from(42_i32)));
            assert!(!sql.contains("= \"entity_key\""), "{sql}");
        }
    }

    /// The rows are owned by the `entities` row through the surrogate key,
    /// so the entity URI appears nowhere in the correlation.
    #[test]
    fn the_correlation_reads_the_surrogate_key_and_never_the_entity_uri() {
        let sql = render(geometry_instances_of(&aliased(), "geo_instance", AttributeNameKey::from(42_i32)));
        assert!(!sql.contains("entity_id"), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    /// Clause 4.10 reads the instances of one named `GeoProperty`, so the scan
    /// restricts on the dictionary key and on the geometry column being
    /// populated: the two halves the supporting partial index is built on.
    #[test]
    fn the_correlation_restricts_to_the_geometry_instances_of_one_attribute() {
        let sql = render(geometry_instances_of(
            &EntityRowRefs::unaliased(None),
            "geo_order",
            AttributeNameKey::from(7_i32),
        ));
        assert!(sql.contains("FROM \"current_attributes\" AS \"geo_order\""), "{sql}");
        assert!(sql.contains("\"geo_order\".\"attribute_name_key\" = 7"), "{sql}");
        assert!(sql.contains("\"geo_order\".\"value_geometry\" IS NOT NULL"), "{sql}");
    }

    /// Two correlated subqueries in one statement read their own alias, so
    /// neither can capture the other's rows.
    #[test]
    fn each_caller_names_its_own_subquery_alias() {
        let first = render(geometry_instances_of(
            &EntityRowRefs::unaliased(None),
            "geo_instance",
            AttributeNameKey::from(1_i32),
        ));
        let second = render(geometry_instances_of(
            &EntityRowRefs::unaliased(None),
            "geo_order",
            AttributeNameKey::from(1_i32),
        ));
        assert!(!first.contains("geo_order"), "{first}");
        assert!(!second.contains("geo_instance"), "{second}");
        assert!(render_column(instance_geometry_column("geo_order")).contains("\"geo_order\".\"value_geometry\""));
    }

    /// Clause 4.7.2 stores an invalid geometry as written; a predicate judges
    /// its repaired form, in the spelling the geometry index holds.
    #[test]
    fn the_instance_geometry_is_the_repaired_form_when_there_is_one() {
        pretty_assertions::assert_eq!(
            render_column(instance_geometry_column("geo_instance")),
            r#"SELECT COALESCE("geo_instance"."value_geometry_valid", "geo_instance"."value_geometry")"#
        );
    }
}
