//! The geometry-valued attribute instances of one attribute, of one entity or
//! of every entity.
//!
//! Clause 4.10 lets a geo-query name any `GeoProperty` of an entity, and clause
//! 4.23 lets a distance ordering measure to one. Every `GeoProperty` other than
//! the default `location` is stored as `current_attributes` rows (one per
//! `datasetId` instance) rather than as a column of `entity_search`, so both the
//! filter and the ordering reach it through the same correlated subquery. That
//! fragment is written once here: a predicate and an ordering that disagreed
//! about which rows a `GeoProperty` lives in would silently order by a geometry
//! they did not filter on. A page that reads the entities a geo-query matches
//! before it reads the entities themselves scans the same instances without
//! the correlation, under the same restrictions.

use crate::{entity_row_refs::EntityRowRefs, standing_instant::instant_of, stored_geometry::StoredGeometry};
use andromeda_ngsi_ld_query::standing_instant::StandingInstant;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{literal::int_literal, postgis::judged_geometry};
use andromeda_postgresql_stored_form::expiry::unexpired;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
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
    scan.from(outer.table(CurrentAttributes::Table).alias(alias.clone())) // clone: the alias names the occurrence and qualifies its columns
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributes::EntityKey)).eq(outer.entity_key_column()));
    restrict_to_geometry_instances(&mut scan, alias, attribute_name_key, outer.standing());
    scan
}

/// A `current_attributes` scan selecting every entity's geometry-valued
/// instances of one attribute in the branch and at the instant `rows` names,
/// left open as [`geometry_instances_of`] is.
///
/// The restriction is the one [`geometry_instances_of`] applies, without the
/// correlation. `value_geometry IS NOT NULL` is the predicate the partial
/// `GiST` index `idx_ca_value_geometry` is restricted by, so a box test on the
/// box it keys reads the instances through it.
#[must_use]
pub fn geometry_instances(rows: &EntityRowRefs, subquery_alias: &str, attribute_name_key: AttributeNameKey) -> SelectStatement {
    let alias = Alias::new(subquery_alias);
    let mut scan = Query::select();
    scan.from(rows.table(CurrentAttributes::Table).alias(alias.clone())); // clone: the alias names the occurrence and qualifies its columns
    restrict_to_geometry_instances(&mut scan, alias, attribute_name_key, rows.standing());
    scan
}

/// Restrict `scan`, reading `current_attributes` as `alias`, to the instances
/// of one attribute that hold a geometry and stand at `standing`.
fn restrict_to_geometry_instances(scan: &mut SelectStatement, alias: Alias, attribute_name_key: AttributeNameKey, standing: StandingInstant) {
    scan.and_where(SeaExpr::col((alias.clone(), CurrentAttributes::AttributeNameKey)).eq(attribute_name_key.get())) // clone: the alias qualifies every column
        .and_where(SeaExpr::col((alias.clone(), CurrentAttributes::ValueGeometry)).is_not_null())
        // Clause 4.22: a past-due instance neither matches nor is measured to.
        .and_where(unexpired(SeaExpr::col((alias, CurrentAttributes::ExpiresAt)), instant_of(standing)));
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

/// A `SELECT 1` marker for a correlated `EXISTS` over one entity's geometry
/// instances, kept beside [`geometry_instances_of`] so every existential over
/// them, in this crate and in the crates that wrap the scan, spells the marker
/// the same way.
#[must_use]
pub fn existence_marker() -> SimpleExpr {
    int_literal(1)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instance_correlation::{existence_marker, geometry_instances_of, instance_geometry_column},
        entity_row_refs::EntityRowRefs,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use sea_query::{Alias, Query, SelectStatement, SimpleExpr};

    /// One correlated scan, given the `SELECT 1` marker each real caller
    /// supplies.
    fn render(mut scan: SelectStatement) -> SelectStatement {
        scan.expr(existence_marker());
        scan
    }

    /// One column expression selected alone.
    fn render_column(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
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
        assert_statement_snapshot!("attribute-instance-correlation-the-correlation-binds-the-outer-row-it-was-given-1", bare);

        let aliased = render(geometry_instances_of(&aliased(), "geo_instance", AttributeNameKey::from(42_i32)));
        assert_statement_snapshot!("attribute-instance-correlation-the-correlation-binds-the-outer-row-it-was-given-2", aliased);
    }

    /// No branch may leave the outer reference unqualified, whatever column it
    /// names: an unqualified reference is the exact shape that silently stops
    /// correlating.
    #[test]
    fn no_correlation_leaves_the_outer_reference_unqualified() {
        for (index, rows) in [EntityRowRefs::unaliased(None), aliased()].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!(
                    "attribute-instance-correlation-no-correlation-leaves-the-outer-reference-unqualified-{}",
                    index + 1
                ),
                render(geometry_instances_of(&rows, "geo_instance", AttributeNameKey::from(42_i32)))
            );
        }
    }

    /// The rows are owned by the `entities` row through the surrogate key,
    /// so the entity URI appears nowhere in the correlation.
    #[test]
    fn the_correlation_reads_the_surrogate_key_and_never_the_entity_uri() {
        assert_statement_snapshot!(
            "attribute-instance-correlation-the-correlation-reads-the-surrogate-key-and-never-the-entity-uri",
            render(geometry_instances_of(&aliased(), "geo_instance", AttributeNameKey::from(42_i32)))
        );
    }

    /// Clause 4.10 reads the instances of one named `GeoProperty`, so the scan
    /// restricts on the dictionary key and on the geometry column being
    /// populated: the two halves the supporting partial index is built on.
    #[test]
    fn the_correlation_restricts_to_the_geometry_instances_of_one_attribute() {
        assert_statement_snapshot!(
            "attribute-instance-correlation-the-correlation-restricts-to-the-geometry-instances-of-one-attribute",
            render(geometry_instances_of(
                &EntityRowRefs::unaliased(None),
                "geo_order",
                AttributeNameKey::from(7_i32),
            ))
        );
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
        assert_statement_snapshot!("attribute-instance-correlation-each-caller-names-its-own-subquery-alias-1", first);
        assert_statement_snapshot!("attribute-instance-correlation-each-caller-names-its-own-subquery-alias-2", second);
        assert_statement_snapshot!(
            "attribute-instance-correlation-each-caller-names-its-own-subquery-alias-3",
            render_column(instance_geometry_column("geo_order"))
        );
    }

    /// Clause 4.7.2 stores an invalid geometry as written; a predicate judges
    /// its repaired form, in the spelling the geometry index holds.
    #[test]
    fn the_instance_geometry_is_the_repaired_form_when_there_is_one() {
        assert_statement_snapshot!(
            "attribute-instance-correlation-the-instance-geometry-is-the-repaired-form-when-there-is-one",
            render_column(instance_geometry_column("geo_instance"))
        );
    }
}
