//! The Attribute instances the value and geo filters of a temporal query keep
//! in its representation (clause 5.7.4.4), rendered over the history rows of
//! one Entity.
//!
//! The rule is [`InstanceRestriction`]'s. An instance of an Attribute that `q`
//! or the geo-query names is kept when the two still hold with that Attribute
//! reduced to that instance, and an instance of any other Attribute as its
//! Entity is, which stage 1 decides.
//!
//! An Entity a Context Source contributed to a split query is judged over its
//! gathered parts, which no statement over one part reads, so its rows are
//! kept here and its instances narrowed in memory once every part is merged.

use crate::{
    alias::Ah,
    value_filters::{exempting_contributed, windowed_instances},
};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, instance_restriction::InstanceRestriction};
use andromeda_postgresql_query::{
    error::RenderError,
    history_source::{HistoryBranch, HistoryOf},
    q_predicate::{instance_scan::InstanceScan, projection as q_filter},
    query_names::QueryNames,
};
use andromeda_postgresql_seaquery::pg_array::is_none_of;
use andromeda_postgresql_stored_form::history_grain::HistoryGrain;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, Expr, ExprTrait};

/// The condition a history row read under [`Ah`] of the Evolution `of` names
/// meets when the query's `q` and geo-query still hold with the row's
/// Attribute reduced to it; `None` when neither names an Attribute the
/// dictionary keyed.
///
/// # Errors
///
/// Returns [`RenderError`] as rendering the geo-query does.
pub(crate) fn met_restrictions(
    branch: &HistoryBranch,
    of: &HistoryOf,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
) -> Result<Option<Condition>, RenderError> {
    let restriction = InstanceRestriction::of(&compiled.entities);
    let reading = restriction.q();
    let geo = restriction.geo();
    let reduced_keys = dictionary_keys(&restriction.narrowed_attributes(), names);
    if reduced_keys.is_empty() {
        return Ok(None);
    }
    let window = windowed_instances(branch, compiled, of);
    let attribute = || Expr::col((Ah, HistoryGrain::AttributeNameKey));
    let mut met = Condition::any().add(is_none_of(attribute(), reduced_keys.iter().map(|key| key.get())));
    for key in reduced_keys {
        let reduced = window.reduced_to(key, Ah);
        let mut holds = Condition::all().add(attribute().eq(key.get()));
        if let Some(reading) = reading {
            holds = holds.add(q_filter::compile(reading, &InstanceScan::History(&reduced), names));
        }
        if let Some(geo) = geo {
            holds = holds.add(reduced.exists_geo(names.attributes.get_str(geo.geoproperty.as_str()), geo)?);
        }
        met = met.add(holds);
    }
    Ok(Some(exempting_contributed(met, &compiled.contributed, of.entity_id_of_row(Ah))))
}

/// The dictionary keys of the narrowed `attributes`, in their order. A name
/// the dictionary never keyed has no history and narrows nothing.
fn dictionary_keys(attributes: &[&AttributeIri], names: &QueryNames) -> Vec<AttributeNameKey> {
    attributes.iter().filter_map(|attribute| names.attributes.get_str(attribute.as_str())).collect()
}

#[cfg(test)]
mod tests {
    use crate::represented_filters::met_restrictions;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        geo::compiled_geo_query::CompiledGeoQuery,
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        temporal::{
            compiled::CompiledTemporalQuery,
            contributed_entities::{ContributedEntities, ContributedRendering},
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SelectStatement, Value};
    use std::str::FromStr;

    const SPEED: &str = "https://example.com/speed";
    const BRAND: &str = "https://example.com/brand";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const PARKED: &str = "https://example.com/parkedIn";

    /// `speed` = 1, `brand` = 2, `location` = 3, `parkedIn` = 4.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [SPEED, BRAND, LOCATION, PARKED]
                    .into_iter()
                    .zip(1..)
                    .map(|(name, key)| (name.parse().expect("valid attribute name"), AttributeNameKey::from(key))),
            ),
            ..QueryNames::default()
        }
    }

    /// A query with a window from noon on `observedAt` and no filter.
    fn compiled() -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(
                TemporalWindow::builder()
                    .timeproperty(TimeProp::Observed)
                    .interval(TimeInterval::After("2020-08-01T12:00:00Z".parse().expect("an instant")))
                    .build(),
            )
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn with_q(q: &str) -> CompiledTemporalQuery {
        let mut compiled = compiled();
        compiled.entities.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded(q.parse::<Filter<Written>>().expect("q parses"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        compiled
    }

    fn with_near() -> CompiledTemporalQuery {
        let mut compiled = compiled();
        compiled.entities.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("2000").expect("a positive distance")),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_text(r#"{"type": "Point", "coordinates": [4.5, 52.5]}"#).expect("a point"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        });
        compiled
    }

    fn rendered(compiled: &CompiledTemporalQuery) -> Option<SelectStatement> {
        met_restrictions(
            &HistoryBranch::live(),
            &HistoryOf::Entity(Expr::val("urn:ngsi-ld:Vehicle:1")),
            compiled,
            &names(),
        )
        .expect("renders")
        .map(|condition| Query::select().expr(Expr::val(1)).cond_where(condition).to_owned())
    }

    /// The last step of clause 5.7.4.4 keeps the instances "which meet the
    /// temporal, query and geoquery restrictions": a row of `speed` is kept
    /// when `q` holds with `speed` reduced to it, and a row of `brand` when
    /// `q` holds with `brand` reduced to it, every other Attribute's rows kept.
    #[test]
    fn a_q_reads_each_attribute_it_names_reduced_to_the_represented_row() {
        let statement = rendered(&with_q(&format!("\"{SPEED}\">50;\"{BRAND}\"==\"Audi\""))).expect("a narrowing");
        assert_statement_snapshot!(
            "represented-filters-a-q-reads-each-attribute-it-names-reduced-to-the-represented-row",
            statement
        );
    }

    /// A term behind a hop keeps the instances of the Relationship it crosses
    /// from the Entity whose linked Entity meets it; the Attribute it ends on
    /// is the linked Entity's, which the representation does not hold.
    #[test]
    fn a_hop_reduces_the_relationship_it_crosses() {
        let statement = rendered(&with_q(&format!("\"{PARKED}\"{{\"{SPEED}\"}}>50"))).expect("a narrowing");
        let (_, values) = statement.build(PostgresQueryBuilder);
        assert_eq!(values.0.get(1), Some(&Value::Int(Some(4))), "the reduced Attribute is parkedIn");
        assert_statement_snapshot!("represented-filters-a-hop-reduces-the-relationship-it-crosses", statement);
    }

    /// Clause 5.7.4.4 keeps the `GeoProperty` instances that meet the
    /// geo-query, read as step S3 reads them.
    #[test]
    fn a_geo_query_reduces_its_geo_property_to_the_represented_row() {
        let statement = rendered(&with_near()).expect("a narrowing");
        assert_statement_snapshot!("represented-filters-a-geo-query-reduces-its-geo-property-to-the-represented-row", statement);
    }

    /// A disjunction across Attributes keeps a row of each when the whole
    /// filter holds with that Attribute reduced to it, the other term read
    /// over the window.
    #[test]
    fn a_disjunction_reduces_each_attribute_it_names() {
        let statement = rendered(&with_q(&format!("\"{SPEED}\">50|\"{BRAND}\"==\"Audi\""))).expect("a narrowing");
        assert_statement_snapshot!("represented-filters-a-disjunction-reduces-each-attribute-it-names", statement);
    }

    /// An inequality is a term like any other: a row of `speed` is kept when
    /// it is unequal to the operand.
    #[test]
    fn an_inequality_reduces_its_attribute_to_the_unequal_rows() {
        let statement = rendered(&with_q(&format!("\"{SPEED}\"!=50"))).expect("a narrowing");
        assert_statement_snapshot!("represented-filters-an-inequality-reduces-its-attribute-to-the-unequal-rows", statement);
    }

    /// A source read for an Evolution by its key reads the same reduction
    /// through the keyed rows.
    #[test]
    fn an_evolution_read_by_its_key_reduces_through_the_keyed_rows() {
        let of = HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        });
        let condition = met_restrictions(&HistoryBranch::live_recorded(), &of, &with_q(&format!("\"{SPEED}\">50")), &names())
            .expect("renders")
            .expect("a narrowing");
        let statement = Query::select().expr(Expr::val(1)).cond_where(condition).to_owned();
        assert_statement_snapshot!("represented-filters-an-evolution-read-by-its-key-reduces-through-the-keyed-rows", statement);
    }

    /// No filter, a Scope query alone and a name the dictionary never keyed
    /// narrow nothing.
    #[test]
    fn no_filter_or_an_unknown_name_narrows_nothing() {
        assert!(rendered(&compiled()).is_none());
        assert!(rendered(&with_q("\"https://example.com/missing\">1")).is_none());
    }

    /// A source read for an Evolution by its key projects no identifier, so a
    /// contributed Entity is named by the identifier the read is narrowed to.
    #[test]
    fn a_contributed_entity_read_by_its_key_is_named_by_the_narrowed_identifier() {
        let mut compiled = with_q(&format!("\"{SPEED}\">50"));
        compiled.contributed = ContributedEntities::new(
            ["urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("an identifier")],
            ContributedRendering::Merged,
        );
        let of = HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        });
        let condition = met_restrictions(&HistoryBranch::live_recorded(), &of, &compiled, &names())
            .expect("renders")
            .expect("a narrowing");
        let statement = Query::select().expr(Expr::val(1)).cond_where(condition).to_owned();
        let text = statement.to_string(PostgresQueryBuilder);
        assert!(!text.contains("\"ah\".\"entity_id\""), "{text}");
        assert_statement_snapshot!(
            "represented-filters-a-contributed-entity-read-by-its-key-is-named-by-the-narrowed-identifier",
            statement
        );
    }

    /// An Entity a Context Source contributed to a split query is judged over
    /// its gathered parts, so its rows are kept.
    #[test]
    fn a_contributed_entity_keeps_every_row() {
        let mut compiled = with_q(&format!("\"{SPEED}\">50"));
        compiled.contributed = ContributedEntities::new(
            ["urn:ngsi-ld:Vehicle:1".parse::<Identifier>().expect("an identifier")],
            ContributedRendering::Merged,
        );
        let statement = rendered(&compiled).expect("a narrowing");
        assert_statement_snapshot!("represented-filters-a-contributed-entity-keeps-every-row", statement);
    }
}
