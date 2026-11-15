//! The Attribute instances the value and geo filters of a temporal query keep
//! in its representation (clause 5.7.4.4).
//!
//! The last step of clause 5.7.4.4 includes only the instances "which meet the
//! temporal, query and geoquery restrictions". An instance of an Attribute
//! that `q` or the geo-query names meets them when the two still hold with that
//! Attribute reduced to that instance; an instance of any other Attribute meets
//! them as its Entity does, which stage 1 decides. A Scope selects Entities
//! only (clauses 4.19 and 5.7.4.4 step S4), so it narrows nothing.
//!
//! An Entity a Context Source contributed to a split query is judged over its
//! gathered parts, which no statement over one part reads, so its instances
//! are kept as the window, `attrs` and `datasetId` keep them.

use crate::{
    alias::Ah,
    value_filters::{exempting_contributed, windowed_instances},
};
use andromeda_ngsi_ld_query::{
    filter::{compiled_filter::CompiledFilter, reading::filter_reading::FilterReading},
    temporal::compiled::CompiledTemporalQuery,
};
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
    let reading = compiled.entities.q.as_ref().map(CompiledFilter::reading);
    let geo = compiled.entities.geo.as_ref();
    let geoproperty = geo.and_then(|geo| names.attributes.get(&geo.geoproperty));
    let reduced_keys = named_attributes(reading, geoproperty, names);
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
            holds = holds.add(reduced.exists_geo(names.attributes.get(&geo.geoproperty), geo)?);
        }
        met = met.add(holds);
    }
    Ok(Some(exempting_contributed(met, &compiled.contributed, Expr::col((Ah, HistoryGrain::EntityId)))))
}

/// The dictionary keys of the Attributes `reading` names and of the geo-query's
/// `GeoProperty` key `geoproperty`, each once: every term's own Attribute, a
/// Relationship a hop crosses from the Entity included. A name the dictionary
/// never keyed has no history and narrows nothing.
fn named_attributes(reading: Option<&FilterReading>, geoproperty: Option<AttributeNameKey>, names: &QueryNames) -> Vec<AttributeNameKey> {
    let mut keys: Vec<AttributeNameKey> = Vec::new();
    let candidates = reading
        .into_iter()
        .flat_map(FilterReading::terms)
        .filter_map(|term| names.attributes.get(term.attribute()))
        .chain(geoproperty);
    for key in candidates {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use crate::represented_filters::met_restrictions;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser},
        geo::query::GeoQuery,
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
        history_source::{HistoryBranch, HistoryOf},
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use sea_query::{Expr, Query, SelectStatement};
    use std::str::FromStr;

    const SPEED: &str = "https://example.com/speed";
    const BRAND: &str = "https://example.com/brand";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// `speed` = 1, `brand` = 2, `location` = 3.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs(
                [SPEED, BRAND, LOCATION]
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
        compiled.entities.q = Some(CompiledFilter::new(parser::parse(q).expect("q parses"), LanguageMatching::TagEquality));
        compiled
    }

    fn with_near() -> CompiledTemporalQuery {
        let mut compiled = compiled();
        compiled.entities.geo = Some(GeoQuery {
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

    /// Clause 5.7.4.4 keeps the `GeoProperty` instances that meet the
    /// geo-query, read as step S3 reads them.
    #[test]
    fn a_geo_query_reduces_its_geo_property_to_the_represented_row() {
        let statement = rendered(&with_near()).expect("a narrowing");
        assert_statement_snapshot!("represented-filters-a-geo-query-reduces-its-geo-property-to-the-represented-row", statement);
    }

    /// No filter, a Scope query alone and a name the dictionary never keyed
    /// narrow nothing.
    #[test]
    fn no_filter_or_an_unknown_name_narrows_nothing() {
        assert!(rendered(&compiled()).is_none());
        assert!(rendered(&with_q("\"https://example.com/missing\">1")).is_none());
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
