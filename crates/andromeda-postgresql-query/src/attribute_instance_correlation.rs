//! Correlating a subquery to one entity's geometry-valued attribute instances.
//!
//! Clause 4.10 lets a geo-query name any GeoProperty of an entity, and clause
//! 4.23 lets a distance ordering measure to one. Every GeoProperty other than
//! the default `location` is stored as `attribute_current` rows — one per
//! `datasetId` instance — rather than as a column of `entities`, so both the
//! filter and the ordering reach it through the same correlated subquery. That
//! fragment is written once here: a predicate and an ordering that disagreed
//! about which rows a GeoProperty lives in would silently order by a geometry
//! they did not filter on.

use crate::attribute_name_id::AttributeNameId;

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

    /// Quote one of the row's columns, carrying its table alias where there is
    /// one: an aliased `e` and `location` give `"e"."location"`, an unaliased
    /// row gives `"location"`.
    pub fn column(&self, column: &str) -> String {
        match self.alias {
            Some(alias) => format!("\"{alias}\".\"{column}\""),
            None => format!("\"{column}\""),
        }
    }
}

/// The `FROM`/`WHERE` fragment selecting one entity's geometry-valued instances
/// of one attribute, written so that a further conjunct can be appended.
///
/// `subquery_alias` names the `attribute_current` occurrence inside the
/// subquery; each caller passes its own so two correlated subqueries nested in
/// one statement cannot capture each other's rows.
///
/// The dictionary key is a broker-minted `SMALLINT` rather than request text, so
/// interpolating it cannot carry an injection. Both of the equality columns and
/// the `value_geo IS NOT NULL` restriction are covered by the partial btree
/// index `idx_ac_attr_entity_geo (attribute_name_id, entity_id)`, so the
/// fragment is an index probe rather than a scan of every instance of the
/// attribute.
pub fn geometry_instances_of(outer: CorrelatedEntityRow<'_>, subquery_alias: &str, attribute_id: AttributeNameId) -> String {
    format!(
        "FROM \"attribute_current\" AS \"{subquery_alias}\" \
         WHERE \"{subquery_alias}\".\"entity_id\" = {} \
           AND \"{subquery_alias}\".\"attribute_name_id\" = {} \
           AND \"{subquery_alias}\".\"value_geo\" IS NOT NULL",
        outer.column("id"),
        attribute_id.get(),
    )
}

/// Quote one of the subquery's geometry columns: the `value_geo` of the
/// instance rows [`geometry_instances_of`] selects.
pub fn instance_geometry_column(subquery_alias: &str) -> String {
    format!("\"{subquery_alias}\".\"value_geo\"")
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_instance_correlation::{CorrelatedEntityRow, geometry_instances_of, instance_geometry_column},
        attribute_name_id::AttributeNameId,
    };

    #[test]
    fn an_alias_qualifies_a_column_and_its_absence_leaves_it_bare() {
        pretty_assertions::assert_eq!(CorrelatedEntityRow::aliased("e").column("location_geo"), "\"e\".\"location_geo\"");
        pretty_assertions::assert_eq!(CorrelatedEntityRow::unaliased().column("location_geo"), "\"location_geo\"");
    }

    /// The correlation names the outer relation explicitly, so a join added to
    /// the enclosing statement cannot silently rebind it.
    #[test]
    fn the_correlation_binds_the_outer_row_it_was_given() {
        let bare = geometry_instances_of(CorrelatedEntityRow::unaliased(), "geo_instance", AttributeNameId::from(42_i16));
        assert!(bare.contains("\"geo_instance\".\"entity_id\" = \"id\""), "{bare}");

        let aliased = geometry_instances_of(CorrelatedEntityRow::aliased("e"), "geo_instance", AttributeNameId::from(42_i16));
        assert!(aliased.contains("\"geo_instance\".\"entity_id\" = \"e\".\"id\""), "{aliased}");
    }

    /// Clause 4.10 reads the instances of one named GeoProperty, so the
    /// fragment restricts on the dictionary key and on the geometry column
    /// being populated — the two halves the supporting partial index is built
    /// on.
    #[test]
    fn the_correlation_restricts_to_the_geometry_instances_of_one_attribute() {
        let sql = geometry_instances_of(CorrelatedEntityRow::unaliased(), "geo_order", AttributeNameId::from(7_i16));
        assert!(sql.starts_with("FROM \"attribute_current\" AS \"geo_order\""), "{sql}");
        assert!(sql.contains("\"geo_order\".\"attribute_name_id\" = 7"), "{sql}");
        assert!(sql.contains("\"geo_order\".\"value_geo\" IS NOT NULL"), "{sql}");
    }

    /// Two correlated subqueries in one statement read their own alias, so
    /// neither can capture the other's rows.
    #[test]
    fn each_caller_names_its_own_subquery_alias() {
        let first = geometry_instances_of(CorrelatedEntityRow::unaliased(), "geo_instance", AttributeNameId::from(1_i16));
        let second = geometry_instances_of(CorrelatedEntityRow::unaliased(), "geo_order", AttributeNameId::from(1_i16));
        assert!(!first.contains("geo_order"), "{first}");
        assert!(!second.contains("geo_instance"), "{second}");
        pretty_assertions::assert_eq!(instance_geometry_column("geo_order"), "\"geo_order\".\"value_geo\"");
    }
}
