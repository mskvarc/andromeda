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

use crate::axis::axis_column;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    error::RenderError,
    history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection as q_filter},
    scope_predicate::{ScopeTarget, condition as scope_condition},
    window_reach::WindowReach,
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::pg_array::is_any_of;
use sea_query::{Condition, Expr as SeaExpr};

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
    if let Some(q_expr) = compiled.entities.q_expr.as_ref() {
        filters = filters.add(q_filter::compile(
            q_expr,
            &InstanceScan::History(&windowed_instances(branch, compiled, &of, WindowReach::InsideAndInEffect)),
            names,
            compiled.entities.language_matching,
        ));
        present = true;
    }
    if let Some(geo) = temporal_geo_existential(branch, compiled, names, &of, WindowReach::InsideAndInEffect)? {
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
    Ok(present.then(|| exempting_contributed(filters, &compiled.contributed)))
}

/// The Temporal Evolution of the stage 1 row `e`, by its identifier and its
/// surrogate key.
fn stage1_evolution() -> HistoryOf {
    HistoryOf::Evolution(EvolutionColumns {
        id: SeaExpr::col((E, Entities::Id)),
        key: SeaExpr::col((E, Entities::EntityKey)),
    })
}

/// `filters`, or an entity a Context Source contributed.
fn exempting_contributed(filters: Condition, contributed: &ContributedEntities) -> Condition {
    if contributed.is_empty() {
        return filters;
    }
    Condition::any().add(filters).add(is_any_of(
        SeaExpr::col((E, Entities::Id)),
        contributed.as_slice().iter().map(Identifier::to_string),
    ))
}

/// The scope-history target a temporal `scopeQ` is read against: a validity
/// interval of the Evolution `of` names overlapping the query's window on its
/// Temporal Property (clause 4.18), among the events `reach` names.
pub(crate) fn scope_history_target(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, of: &HistoryOf, reach: WindowReach) -> ScopeTarget {
    ScopeTarget::history(
        of.clone(), // clone: the target owns the Evolution its probe reads
        compiled.window.timeproperty,
        compiled.window.interval,
        branch.clone(), // clone: the target owns the branch its probe reads
        reach,
    )
}

/// The Attribute instances a temporal `q` is checked against (clause 5.7.4.4
/// step S2): the history rows of the Evolution `of` names valid within the
/// window on the requested Temporal Property, of every `datasetId`, those
/// `reach` names.
pub(crate) fn windowed_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, of: &HistoryOf, reach: WindowReach) -> HistoryWindow {
    HistoryWindow::builder()
        .branch(branch.clone()) // clone: the window owns the branch its probes read
        .of(of.clone()) // clone: the window owns the Evolution its probes read
        .axis(axis_column(compiled.window.timeproperty))
        .interval(compiled.window.interval)
        .reach(reach)
        .build()
}

/// Whether one `GeoProperty` instance of the Evolution `of` names, valid
/// within the temporal window, meets the query's geo restriction (clause 4.10;
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
    reach: WindowReach,
) -> Result<Option<Condition>, RenderError> {
    let Some(geo) = &compiled.entities.geo else {
        return Ok(None);
    };
    windowed_instances(branch, compiled, of, reach)
        .exists_geo(names.attributes.get(&geo.geoproperty), geo)
        .map(Some)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        split_filter_leaves::build_filter_leaf_truths,
        value_filters::{stage1_evolution, temporal_geo_existential, value_filter_condition},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser,
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_type_name_key::EntityTypeNameKey,
        history_source::HistoryBranch,
        window_reach::WindowReach,
    };
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query, Value, Values};
    use std::str::FromStr;

    const TEMPERATURE: &str = "https://uri.etsi.org/ngsi-ld/default-context/temperature";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const SENSOR: &str = "https://uri.etsi.org/ngsi-ld/default-context/Sensor";

    /// A probe's source compared beside nothing: narrowed inside to the
    /// Evolution's surrogate key.
    const BY_KEY: &str = r#""rs"."entity_key" = "e"."entity_key""#;
    /// The Scope source narrowed inside to the Evolution's surrogate key.
    const SCOPE_BY_KEY: &str = r#""rsh"."entity_key" = "e"."entity_key""#;
    /// A recorded source narrowed inside to the Evolution's identifier.
    const BY_IDENTIFIER: &str = r#""re"."id" = "e"."id""#;
    /// The geo probe's comparison of the `GeoProperty`'s dictionary key.
    const GEO_KEY: &str = r#""qh"."attribute_name_key" = $"#;

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
                        EntitySelector::builder()
                            .type_expr(Some(TypeExpr::Type(IriBuf::new(SENSOR.to_owned()).expect("valid IRI"))))
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
        compiled.entities.q_expr = Some(parser::parse(&format!("\"{TEMPERATURE}\">99")).expect("q parses"));
        compiled
    }

    /// `georel=near;maxDistance==2000` around 4.5, 52.5 on `location`.
    fn near(mut compiled: CompiledTemporalQuery) -> CompiledTemporalQuery {
        compiled.entities.geo = Some(GeoQuery {
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
        compiled.entities.geo = Some(GeoQuery {
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
        compiled.entities.scope_q = Some(ScopeQuery::parse("/city/moved1").expect("valid scope query"));
        compiled
    }

    /// The value, geo and scope filters stage 1 conjoins, alone.
    fn filters(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> String {
        let condition = value_filter_condition(branch, compiled, &names()).expect("renders").expect("a filter");
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// The geo filter of `compiled` alone, with its bound values.
    fn geo_filter(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, names: &QueryNames) -> (String, Values) {
        let condition = temporal_geo_existential(branch, compiled, names, &stage1_evolution(), WindowReach::InsideAndInEffect)
            .expect("renders")
            .expect("a geo query");
        Query::select().expr(Expr::val(1)).cond_where(condition).to_owned().build(PostgresQueryBuilder)
    }

    /// The geo probe stage 1 conjoins, with its bound values.
    fn geo_probe(branch: &HistoryBranch, names: &QueryNames) -> (String, Values) {
        geo_filter(branch, &near(compiled(before_2030())), names)
    }

    /// The value bound to the placeholder right after the first `fragment`.
    fn bound_after(sql: &str, values: &Values, fragment: &str) -> Option<Value> {
        let start = sql.find(fragment)? + fragment.len();
        let digits: String = sql[start..].chars().take_while(char::is_ascii_digit).collect();
        let position = digits.parse::<usize>().ok()?.checked_sub(1)?;
        values.0.get(position).cloned()
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
        let (sql, values) = geo_probe(&HistoryBranch::live_recorded(), &names());
        assert_eq!(bound_after(&sql, &values, GEO_KEY), Some(Value::Int(Some(2))), "{sql}");
        assert!(!sql.contains("attribute_names"), "{sql}");
    }

    #[test]
    fn the_temporal_near_probe_prefilters_by_an_unindexed_box() {
        let (sql, _) = geo_probe(&HistoryBranch::live_recorded(), &names());
        let unindexed = sql
            .find(r#"COALESCE("qh"."value_geometry_reach", "qh"."value_geometry_valid", "qh"."value_geometry")::box2d && ST_MakeEnvelope("#)
            .expect("the unindexed box");
        let distance = sql.find("ST_DWithin").expect("the distance");
        assert!(unindexed < distance, "{sql}");
        assert!(
            sql[distance..].starts_with(r#"ST_DWithin(COALESCE("qh"."value_geometry_valid", "qh"."value_geometry")::geography, "#),
            "{sql}"
        );
        assert!(sql[unindexed..distance].contains(")::box2d"), "{sql}");
        assert!(!sql.contains(r#""qh"."value_geometry") && ST_MakeEnvelope("#), "{sql}");
    }

    /// An area relation is prefiltered too, by the reference's box against
    /// the rows' reach box, which no geometry index serves on this probe.
    #[test]
    fn an_area_relation_carries_an_unindexed_box_prefilter() {
        for georel in [GeoRel::Within, GeoRel::Intersects] {
            let (sql, _) = geo_filter(&HistoryBranch::live_recorded(), &area(georel, compiled(before_2030())), &names());
            assert!(
                sql.contains(
                    r#"COALESCE("qh"."value_geometry_reach", "qh"."value_geometry_valid", "qh"."value_geometry")::box2d && ST_MakeValid(ST_GeomFromEWKB("#
                ),
                "{sql}"
            );
        }
    }

    /// A `GeoProperty` the dictionary never keyed has no history, so the
    /// probe matches no instance, as the window match does for an attribute
    /// never keyed.
    #[test]
    fn an_unrecorded_geoproperty_matches_no_instance() {
        let (sql, _) = geo_probe(&HistoryBranch::live_recorded(), &QueryNames::default());
        assert!(sql.ends_with("WHERE FALSE"), "{sql}");
        assert!(!sql.contains("attribute_names") && !sql.contains(r#""qh""#), "{sql}");
    }

    #[test]
    fn a_split_geo_leaf_compares_by_dictionary_key() {
        let (sql, values) = build_filter_leaf_truths(&HistoryBranch::live_recorded(), &near(compiled(before_2030())), &vehicles(), &names())
            .expect("renders")
            .build(PostgresQueryBuilder);
        assert_eq!(bound_after(&sql, &values, GEO_KEY), Some(Value::Int(Some(2))), "{sql}");
        assert!(!sql.contains("attribute_names"), "{sql}");
    }

    /// The key is compared once on the source's own grain column, above the
    /// union of the recorded part and the queued part: PostgreSQL pushes a
    /// restriction on a derived table into every branch of it.
    #[test]
    fn a_queued_branch_compares_the_key_inside_each_part() {
        let (sql, values) = geo_probe(&HistoryBranch::live(), &names());
        let recorded = sql.find(r#"FROM "attribute_history" AS "rh""#).expect("the recorded part");
        let queued = sql.find(r#"FROM "temporal_append_entries" AS "pe""#).expect("the queued part");
        let key = sql.find(&format!(r#") AS "qh" WHERE {GEO_KEY}"#)).expect("the key comparison on the source");
        assert!(recorded < queued && queued < key, "{sql}");
        assert_eq!(sql.matches(GEO_KEY).count(), 1, "{sql}");
        assert_eq!(bound_after(&sql, &values, GEO_KEY), Some(Value::Int(Some(2))), "{sql}");
        assert!(!sql.contains("attribute_names"), "{sql}");
    }

    #[test]
    fn a_recorded_branch_correlates_the_q_probe_by_evolution_key() {
        let sql = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(before_2030())));
        assert!(sql.contains(&format!(r#"{BY_KEY}) AS "qh""#)), "{sql}");
        assert!(!sql.contains(BY_IDENTIFIER) && !sql.contains("entities"), "{sql}");
    }

    /// Clause 4.11 with Annex C clause C.5.5: an `after` window also reads the
    /// instance in effect when it opens, through the stream source narrowed
    /// by the key and the rows of each stream read by its stream key.
    #[test]
    fn a_recorded_branch_correlates_the_in_effect_instance_by_evolution_key() {
        let sql = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(after_2019())));
        for alias in ["qh", "qs"] {
            assert!(sql.contains(&format!(r#"{BY_KEY}) AS "{alias}""#)), "{alias}: {sql}");
        }
        assert!(sql.contains(r#""rh"."stream_key" = "qs"."stream_key") AS "qi""#), "{sql}");
        assert!(sql.contains(r#""qs"."entity_key" AS "entity_key""#), "{sql}");
        assert!(!sql.contains(BY_IDENTIFIER) && !sql.contains("entities"), "{sql}");
    }

    #[test]
    fn a_recorded_branch_correlates_the_geo_probe_by_evolution_key() {
        let sql = filters(&HistoryBranch::live_recorded(), &near(compiled(before_2030())));
        assert!(sql.contains(&format!(r#"{BY_KEY}) AS "qh""#)), "{sql}");
        assert!(!sql.contains(BY_IDENTIFIER) && !sql.contains("entities"), "{sql}");
    }

    /// Clause 5.7.4.4 steps S2 and S3 read the same instances: for one window,
    /// `q` and `georel` both probe the instances inside it and the instance in
    /// effect when it opens (the one placed on its stream, `qi`), and neither
    /// counts a deletion.
    #[test]
    fn q_and_georel_read_the_same_window_instances() {
        let q = filters(&HistoryBranch::live_recorded(), &rare_q(compiled(after_2019())));
        let (geo, _) = geo_filter(&HistoryBranch::live_recorded(), &near(compiled(after_2019())), &names());
        for (filter, sql) in [("q", &q), ("georel", &geo)] {
            assert!(sql.contains(r#" AS "qi""#), "{filter} reads the instance in effect: {sql}");
            assert!(sql.contains(r#""qh"."observed_at" >"#), "{filter} reads the instances inside: {sql}");
            assert!(sql.contains(r#""qh"."value_kind" <> "#), "{filter} skips a deletion: {sql}");
        }
    }

    /// Clause 4.18: an `after` window also asks whether a later event of the
    /// same Evolution superseded the Scope, which names it by the key too.
    #[test]
    fn a_recorded_branch_correlates_the_scope_probe_by_evolution_key() {
        let sql = filters(&HistoryBranch::live_recorded(), &moved(compiled(before_2030())));
        assert!(sql.contains(&format!(r#"{SCOPE_BY_KEY}) AS "sh""#)), "{sql}");
        assert!(!sql.contains("entities") && !sql.contains("entity_id"), "{sql}");
        let after = filters(&HistoryBranch::live_recorded(), &moved(compiled(after_2019())));
        assert!(after.contains(r#""rsh"."entity_key" = "sh"."entity_key") AS "sh_next""#), "{after}");
        assert!(!after.contains("entities") && !after.contains("entity_id"), "{after}");
    }

    #[test]
    fn a_snapshot_branch_correlates_by_evolution_key() {
        let sql = filters(&snapshot(), &moved(near(rare_q(compiled(before_2030())))));
        for (relation, correlation) in [
            (r#""snap_7"."attribute_history" AS "rh""#, format!(r#"{BY_KEY}) AS "qh""#)),
            (r#""snap_7"."scope_history" AS "rsh""#, format!(r#"{SCOPE_BY_KEY}) AS "sh""#)),
        ] {
            assert!(sql.contains(relation), "{relation}: {sql}");
            assert!(sql.contains(&correlation), "{correlation}: {sql}");
        }
        assert!(!sql.contains(BY_IDENTIFIER) && !sql.contains("entities"), "{sql}");
    }

    /// A queued entry carries no surrogate key, so a branch reading its queue
    /// narrows every part of every probe's source by the identifier.
    #[test]
    fn a_queued_branch_keeps_the_identifier_correlation() {
        let sql = filters(&HistoryBranch::live(), &moved(near(rare_q(compiled(after_2019())))));
        assert_eq!(
            sql.matches(BY_IDENTIFIER).count(),
            5,
            "the inside and stream sources of q and of the geo-query, and the Scope source: {sql}"
        );
        assert!(sql.contains(r#""pe"."entity_id" = "e"."id""#), "{sql}");
        assert!(sql.contains(r#""re"."id" = "sh"."entity_id""#), "{sql}");
        assert!(!sql.contains(r#""entity_key" = "e"."entity_key""#), "{sql}");
    }

    /// A split query reads its leaves for a list of identifiers, so every
    /// leaf names the Evolution by its identifier, over recorded rows alone
    /// too.
    #[test]
    fn a_split_leaf_keeps_the_identifier() {
        let sql = build_filter_leaf_truths(
            &HistoryBranch::live_recorded(),
            &moved(near(rare_q(compiled(before_2030())))),
            &vehicles(),
            &names(),
        )
        .expect("renders")
        .to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches(BY_IDENTIFIER).count(), 3, "the q, geo and Scope leaves: {sql}");
        assert!(!sql.contains(r#""entity_key" = "e"."entity_key""#), "{sql}");
    }
}
