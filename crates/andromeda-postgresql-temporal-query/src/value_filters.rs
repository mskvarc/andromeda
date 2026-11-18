//! The value, geo and scope filters of a temporal query, rendered against the
//! temporal catalogue row stage 1 selects under the `e` alias.
//!
//! Clause 5.7.4.4 checks `q` against "all the Attribute instances resulting from
//! the initial filtering performed by the temporal query", the geoquery against
//! "the `GeoProperty` instances that are within the interval defined by the
//! temporal query", and the scope query against the Entity's Scope instances in
//! that interval. Each is a correlated existential over the history of the
//! temporal entity `e` names. Stage 1 conjoins them into its selection, and a
//! split query reads each leaf of them on its own (`crate::split_filter_leaves`),
//! so both render the same leaves from here.
//!
//! Every walk stage 1 reads carries the Evolution's surrogate key beside its
//! identifier, so stage 1 reads each probe's history for the Evolution by
//! both ([`HistoryOf::Evolution`]): over recorded rows alone the probe
//! compares the key every recorded stream and Scope event carries, with no
//! catalogue row looked up per probe. A split query reads its leaves for a
//! list of identifiers and names the Evolution by its identifier.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities};
use andromeda_postgresql_query::{
    error::RenderError,
    history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection as q_filter},
    query_names::QueryNames,
    scope_predicate::{ScopeTarget, condition as scope_condition},
    window_reach::WindowReach,
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::pg_array::is_any_of;
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// The conjunction of the query's `q`, geo and scope filters over the temporal
/// entity `e`, with the entities a Context Source contributed to a split query
/// exempt from it; `None` when the query carries none of the three.
///
/// Clause 5.7.4.4 applies the three filters to an entity held in several parts
/// only once the parts are aggregated. An entity no source answered with has no
/// part elsewhere, so the filters decide it here; a contributed one passes, and
/// the parts are judged together once they are gathered.
///
/// # Errors
///
/// Returns [`RenderError`] exactly as [`temporal_geo_existential`] does.
pub(crate) fn value_filter_condition(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Result<Option<Condition>, RenderError> {
    let of = stage1_evolution();
    let mut filters = Condition::all();
    let mut present = false;
    if let Some(q) = compiled.entities.q.as_ref() {
        filters = filters.add(q_filter::compile(
            q.reading(),
            &InstanceScan::History(&windowed_instances(branch, compiled, &of)),
            names,
        ));
        present = true;
    }
    if let Some(geo) = temporal_geo_existential(branch, compiled, names, &of)? {
        filters = filters.add(geo);
        present = true;
    }
    if let Some(scope_q) = &compiled.entities.scope_q {
        filters = filters.add(scope_condition(
            scope_q,
            &scope_history_target(branch, compiled, &of, WindowReach::InsideAndInEffect),
        ));
        present = true;
    }
    Ok(present.then(|| exempting_contributed(filters, &compiled.contributed, SeaExpr::col((E, Entities::Id)))))
}

/// The Temporal Evolution of the stage 1 row `e`, by its identifier and its
/// surrogate key.
fn stage1_evolution() -> HistoryOf {
    HistoryOf::Evolution(EvolutionColumns {
        id: SeaExpr::col((E, Entities::Id)),
        key: SeaExpr::col((E, Entities::EntityKey)),
    })
}

/// `filters`, or the entity identified by `entity_id` being one a Context
/// Source contributed.
pub(crate) fn exempting_contributed(filters: Condition, contributed: &ContributedEntities, entity_id: SimpleExpr) -> Condition {
    if contributed.is_empty() {
        return filters;
    }
    Condition::any()
        .add(filters)
        .add(is_any_of(entity_id, contributed.as_slice().iter().map(Identifier::to_string)))
}

/// The scope-history target a temporal `scopeQ` is read against: a validity
/// interval of the Evolution `of` names overlapping the query's window on its
/// Temporal Property (clause 4.18), among the events `reach` names.
pub(crate) fn scope_history_target(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, of: &HistoryOf, reach: WindowReach) -> ScopeTarget {
    ScopeTarget::history(of.clone(), compiled.window.timeproperty, compiled.window.interval, branch.clone(), reach)
}

/// The Attribute instances a temporal `q` is checked against (clause 5.7.4.4
/// step S2): the history rows of the Evolution `of` names inside the
/// window on the requested Temporal Property, of every `datasetId`.
pub(crate) fn windowed_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, of: &HistoryOf) -> HistoryWindow {
    HistoryWindow::builder()
        .branch(branch.clone())
        .of(of.clone())
        .timeproperty(compiled.window.timeproperty)
        .interval(compiled.window.interval)
        .build()
}

/// Whether one `GeoProperty` instance of the Evolution `of` names, inside
/// the temporal window, meets the query's geo restriction (clause 4.10;
/// clause 5.7.4.4 step S3); `None` when the query carries none.
///
/// Every instance of the target `GeoProperty`, the default `location` and any
/// named one alike, is a history row carrying a `value_geometry`, so the probe
/// reaches them uniformly through the Evolution's history rather than through
/// the single hoisted current-state `location` column, which holds only one
/// instant and answers a different question. The instances are the window's
/// own, read exactly as the `q` filter reads them.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] or [`RenderError::Query`]
/// exactly as the shared relation rendering does.
pub(crate) fn temporal_geo_existential(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    of: &HistoryOf,
) -> Result<Option<Condition>, RenderError> {
    let Some(geo) = &compiled.entities.geo else {
        return Ok(None);
    };
    windowed_instances(branch, compiled, of)
        .exists_geo(names.attributes.get(&geo.geoproperty), geo)
        .map(Some)
}

#[cfg(test)]
mod tests {
    use crate::{
        split_filter_leaves::build_filter_leaf_truths,
        value_filters::{stage1_evolution, temporal_geo_existential, value_filter_condition},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        expanded_entity_selector::ExpandedEntitySelector,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        geo::compiled_geo_query::CompiledGeoQuery,
        scope::scope_query::ScopeQuery,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
        type_selection::expanded_type_selection::ExpandedTypeSelection,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{history_source::HistoryBranch, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use sea_query::{Alias, Expr, IntoIden, Query, SelectStatement};
    use std::str::FromStr;

    const TEMPERATURE: &str = "https://uri.etsi.org/ngsi-ld/default-context/temperature";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const SENSOR: &str = "https://uri.etsi.org/ngsi-ld/default-context/Sensor";

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

    fn before_2030() -> TimeInterval {
        TimeInterval::Before(instant("2030-01-01T00:00:00Z"))
    }

    fn after_2019() -> TimeInterval {
        TimeInterval::After(instant("2019-01-01T00:00:00Z"))
    }

    /// `Sensor` = type key 1, `temperature` = 1, `location` = 2.
    fn names() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::from_pairs([
                (TEMPERATURE.parse().expect("valid attribute name"), AttributeNameKey::from(1)),
                (LOCATION.parse().expect("valid attribute name"), AttributeNameKey::from(2)),
            ]),
            types: ResolvedNames::from_pairs([(SENSOR.parse().expect("valid type name"), EntityTypeNameKey::from(1))]),
            ..QueryNames::default()
        }
    }

    /// A query over the type `Sensor` with a window on `observedAt`.
    fn compiled(interval: TimeInterval) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(
                EntitySelection::builder()
                    .selectors(vec![
                        ExpandedEntitySelector::builder()
                            .type_selection(Some(ExpandedTypeSelection::Type(IriBuf::new(SENSOR.to_owned()).expect("valid IRI"))))
                            .build(),
                    ])
                    .build(),
            )
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).interval(interval).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    /// `q=temperature>99`.
    fn rare_q(mut compiled: CompiledTemporalQuery) -> CompiledTemporalQuery {
        compiled.entities.q = Some(CompiledFilter::new(
            format!("\"{TEMPERATURE}\">99").parse::<Filter>().expect("q parses"),
            LanguageMatching::TagEquality,
        ));
        compiled
    }

    /// `georel=near;maxDistance==2000` around 4.5, 52.5 on `location`.
    fn near(mut compiled: CompiledTemporalQuery) -> CompiledTemporalQuery {
        compiled.entities.geo = Some(CompiledGeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("2000").expect("positive distance")),
                min_distance: None,
            },
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({"type": "Point", "coordinates": [4.5, 52.5]})).expect("point"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        });
        compiled
    }

    /// `georel` with a small square around 4.5, 52.5 on `location`.
    fn area(georel: GeoRel, mut compiled: CompiledTemporalQuery) -> CompiledTemporalQuery {
        compiled.entities.geo = Some(CompiledGeoQuery {
            georel,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({
                "type": "Polygon",
                "coordinates": [[[4.49, 52.49], [4.51, 52.49], [4.51, 52.51], [4.49, 52.51], [4.49, 52.49]]],
            }))
            .expect("polygon"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        });
        compiled
    }

    /// `scopeQ=/city/moved1`.
    fn moved(mut compiled: CompiledTemporalQuery) -> CompiledTemporalQuery {
        compiled.entities.scope_q = Some("/city/moved1".parse::<ScopeQuery>().expect("valid scope query"));
        compiled
    }

    /// The value, geo and scope filters stage 1 conjoins, alone.
    fn filters(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> SelectStatement {
        let condition = value_filter_condition(branch, compiled, &names()).expect("renders").expect("a filter");
        Query::select().expr(Expr::val(1)).cond_where(condition).to_owned()
    }

    /// The geo filter of `compiled` alone.
    fn geo_filter(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> SelectStatement {
        let condition = temporal_geo_existential(branch, compiled, names, &stage1_evolution())
            .expect("renders")
            .expect("a geo query");
        Query::select().expr(Expr::val(1)).cond_where(condition).to_owned()
    }

    /// The geo probe stage 1 conjoins.
    fn geo_probe(branch: &HistoryBranch, names: &QueryNames) -> SelectStatement {
        geo_filter(branch, &near(compiled(before_2030())), names)
    }

    fn vehicles() -> Vec<Identifier> {
        ["urn:ngsi-ld:Sensor:000001", "urn:ngsi-ld:Sensor:000002"]
            .iter()
            .map(|id| id.parse().expect("valid identifier"))
            .collect()
    }

    fn snapshot() -> HistoryBranch {
        HistoryBranch::recorded_in(Alias::new("snap_7").into_iden())
    }

    #[test]
    fn the_geo_probe_compares_the_geoproperty_by_dictionary_key() {
        let statement = geo_probe(&HistoryBranch::live_recorded(), &names());
        assert_statement_snapshot!("value-filters-the-geo-probe-compares-the-geoproperty-by-dictionary-key", statement);
    }

    #[test]
    fn the_temporal_near_probe_prefilters_by_an_unindexed_box() {
        let statement = geo_probe(&HistoryBranch::live_recorded(), &names());
        assert_statement_snapshot!("value-filters-the-temporal-near-probe-prefilters-by-an-unindexed-box", statement);
    }

    /// An area relation is prefiltered too, by the reference's box against
    /// the rows' reach box, which no geometry index serves on this probe.
    #[test]
    fn an_area_relation_carries_an_unindexed_box_prefilter() {
        let within = geo_filter(&HistoryBranch::live_recorded(), &area(GeoRel::Within, compiled(before_2030())), &names());
        assert_statement_snapshot!("value-filters-an-area-relation-carries-an-unindexed-box-prefilter-1", within);
        let intersects = geo_filter(&HistoryBranch::live_recorded(), &area(GeoRel::Intersects, compiled(before_2030())), &names());
        assert_statement_snapshot!("value-filters-an-area-relation-carries-an-unindexed-box-prefilter-2", intersects);
    }

    /// A `GeoProperty` the dictionary never keyed has no history, so the
    /// probe matches no instance, as the window match does for an attribute
    /// never keyed.
    #[test]
    fn an_unrecorded_geoproperty_matches_no_instance() {
        let statement = geo_probe(&HistoryBranch::live_recorded(), &QueryNames::default());
        assert_statement_snapshot!("value-filters-an-unrecorded-geoproperty-matches-no-instance", statement);
    }

    #[test]
    fn a_split_geo_leaf_compares_by_dictionary_key() {
        let statement = build_filter_leaf_truths(&HistoryBranch::live_recorded(), &near(compiled(before_2030())), &vehicles(), &names()).expect("renders");
        assert_statement_snapshot!("value-filters-a-split-geo-leaf-compares-by-dictionary-key", statement);
    }

    /// The key is compared once on the source's own grain column, above the
    /// union of the recorded part and the queued part: PostgreSQL pushes a
    /// restriction on a derived table into every branch of it.
    #[test]
    fn a_queued_branch_compares_the_key_inside_each_part() {
        let statement = geo_probe(&HistoryBranch::live(), &names());
        assert_statement_snapshot!("value-filters-a-queued-branch-compares-the-key-inside-each-part", statement);
    }

    #[test]
    fn a_recorded_branch_correlates_the_q_probe_by_evolution_key() {
        let statement = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(before_2030())));
        assert_statement_snapshot!("value-filters-a-recorded-branch-correlates-the-q-probe-by-evolution-key", statement);
    }

    /// Clause 5.7.4.4 step S2: an `after` window reads the instances inside it
    /// alone, none its streams held when it opened.
    #[test]
    fn an_after_window_reads_the_q_instances_inside_it_alone() {
        let statement = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(after_2019())));
        assert_statement_snapshot!("value-filters-an-after-window-reads-the-q-instances-inside-it-alone", statement);
    }

    #[test]
    fn a_recorded_branch_correlates_the_geo_probe_by_evolution_key() {
        let statement = filters(&HistoryBranch::live_recorded(), &near(compiled(before_2030())));
        assert_statement_snapshot!("value-filters-a-recorded-branch-correlates-the-geo-probe-by-evolution-key", statement);
    }

    /// Clause 5.7.4.4 steps S2 and S3 read the same instances: for one window,
    /// `q` and `georel` both probe the instances inside it, and neither counts
    /// a deletion.
    #[test]
    fn q_and_georel_read_the_same_window_instances() {
        let q = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(after_2019())));
        assert_statement_snapshot!("value-filters-q-and-georel-read-the-same-window-instances-1", q);
        let geo = geo_filter(&HistoryBranch::live_recorded(), &near(compiled(after_2019())), &names());
        assert_statement_snapshot!("value-filters-q-and-georel-read-the-same-window-instances-2", geo);
    }

    /// Clause 4.18: an `after` window also asks whether a later event of the
    /// same Evolution superseded the Scope, which names it by the key too.
    #[test]
    fn a_recorded_branch_correlates_the_scope_probe_by_evolution_key() {
        let before = filters(&HistoryBranch::live_recorded(), &moved(compiled(before_2030())));
        assert_statement_snapshot!("value-filters-a-recorded-branch-correlates-the-scope-probe-by-evolution-key-1", before);
        let after = filters(&HistoryBranch::live_recorded(), &moved(compiled(after_2019())));
        assert_statement_snapshot!("value-filters-a-recorded-branch-correlates-the-scope-probe-by-evolution-key-2", after);
    }

    #[test]
    fn a_snapshot_branch_correlates_by_evolution_key() {
        let statement = filters(&snapshot(), &moved(near(rare_q(compiled(before_2030())))));
        assert_statement_snapshot!("value-filters-a-snapshot-branch-correlates-by-evolution-key", statement);
    }

    /// A queued entry carries no surrogate key, so a branch reading its queue
    /// narrows every part of every probe's source by the identifier.
    #[test]
    fn a_queued_branch_keeps_the_identifier_correlation() {
        let statement = filters(&HistoryBranch::live(), &moved(near(rare_q(compiled(after_2019())))));
        assert_statement_snapshot!("value-filters-a-queued-branch-keeps-the-identifier-correlation", statement);
    }

    /// A split query reads its leaves for a list of identifiers, so every
    /// leaf names the Evolution by its identifier, over recorded rows alone
    /// too.
    #[test]
    fn a_split_leaf_keeps_the_identifier() {
        let statement = build_filter_leaf_truths(
            &HistoryBranch::live_recorded(),
            &moved(near(rare_q(compiled(before_2030())))),
            &vehicles(),
            &names(),
        )
        .expect("renders");
        assert_statement_snapshot!("value-filters-a-split-leaf-keeps-the-identifier", statement);
    }
}
