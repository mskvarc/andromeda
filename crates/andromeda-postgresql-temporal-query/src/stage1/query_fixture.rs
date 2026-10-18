//! The queries, dictionaries and renderings the stage 1 tests share.

use crate::stage1::{Stage1Query, build_stage1_entity_ids};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity_selection::{EntitySelection, EntitySelector},
    page_window::PageWindow,
    pattern::Pattern,
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
};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sea_query::{PostgresQueryBuilder, Values};

/// The UTC instant `raw` spells in RFC 3339.
pub(super) fn instant(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
}

/// The identifiers `raw` spells.
pub(super) fn ids(raw: &[&str]) -> Vec<Identifier> {
    raw.iter().map(|id| id.parse().expect("valid URI")).collect()
}

/// The IRI `value` spells.
pub(super) fn iri(value: &str) -> IriBuf {
    IriBuf::new(value.to_string()).expect("valid IRI")
}

/// The type dictionary with the one type the selector tests name.
pub(super) fn vehicle_is_known() -> QueryNames {
    QueryNames {
        attributes: ResolvedNames::default(),
        types: ResolvedNames::from_pairs([(
            "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
            EntityTypeNameKey::from(3),
        )]),
    }
}

/// A `GeoJSON` point reference geometry, for tests that vary the relation
/// rather than the geometry.
pub(super) fn point(coordinates: &serde_json::Value) -> Geometry {
    serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
}

/// A query with every filter absent, for tests that vary one part at a time.
pub(super) fn minimal_compiled() -> CompiledTemporalQuery {
    CompiledTemporalQuery::builder()
        .entities(EntitySelection::builder().build())
        .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
        .instances(InstanceSelection::builder().build())
        .representation(
            Representation::builder()
                .format(TemporalFormat::Normalized)
                .sys_attrs(SysAttrsInclusion::Strip)
                .build(),
        )
        .build()
}

/// The Stage 1 result statement, rendered with `$N` placeholders and its
/// bound values, which is what the store hands to the driver.
pub(super) fn built(compiled: &CompiledTemporalQuery, names: &QueryNames) -> (String, Values) {
    built_windowed(compiled, &PageWindow::offset(20, 0), names)
}

/// The Stage 1 result statement of the page `window` selects.
pub(super) fn built_windowed(compiled: &CompiledTemporalQuery, window: &PageWindow, names: &QueryNames) -> (String, Values) {
    build_stage1_entity_ids(&HistoryBranch::live(), compiled, window, CountPolicy::Omit, names)
        .expect("valid temporal query")
        .select
        .build(PostgresQueryBuilder)
}

/// The count statement of a counted query.
pub(super) fn built_count(compiled: &CompiledTemporalQuery, names: &QueryNames) -> String {
    build_stage1_entity_ids(&HistoryBranch::live(), compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, names)
        .expect("valid temporal query")
        .count
        .expect("count requested")
        .build(PostgresQueryBuilder)
        .0
}

/// The `Vehicle` type the selector tests name.
pub(super) fn vehicle() -> TypeExpr {
    TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"))
}

/// A query with every filter absent but the entity selectors `selectors`.
pub(super) fn typed(selectors: Vec<EntitySelector>) -> CompiledTemporalQuery {
    let mut compiled = minimal_compiled();
    compiled.entities.selectors = selectors;
    compiled
}

/// A selector of the `Vehicle` type alone.
pub(super) fn vehicle_selector() -> EntitySelector {
    EntitySelector::builder().type_expr(Some(vehicle())).build()
}

/// The attribute names the `attrs`, `q` and window tests resolve: `speed` at
/// key 7, `address` at 9.
pub(super) fn speed_and_address() -> QueryNames {
    QueryNames {
        attributes: ResolvedNames::from_pairs([
            ("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameKey::from(7)),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(9)),
        ]),
        types: ResolvedNames::default(),
    }
}

/// The names the step tests resolve: the type `Sensor` at key 1,
/// `temperature` at 1 and `location` at 2.
pub(super) fn sensor_names() -> QueryNames {
    QueryNames {
        attributes: ResolvedNames::from_pairs([
            (
                "https://uri.etsi.org/ngsi-ld/default-context/temperature"
                    .parse()
                    .expect("valid attribute name"),
                AttributeNameKey::from(1),
            ),
            (
                "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
                AttributeNameKey::from(2),
            ),
        ]),
        types: ResolvedNames::from_pairs([(
            "https://uri.etsi.org/ngsi-ld/default-context/Sensor".parse().expect("valid type name"),
            EntityTypeNameKey::from(1),
        )]),
    }
}

/// A query of the `Sensor` type over the window `before
/// 2030-01-01T00:00:00Z` on `observedAt`, every other filter absent.
pub(super) fn sensors() -> CompiledTemporalQuery {
    let mut compiled = typed(vec![
        EntitySelector::builder()
            .type_expr(Some(TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Sensor"))))
            .build(),
    ]);
    compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
    compiled
}

/// A query of the selector `idPattern=<raw>`, of the type `ty` of the default
/// context when one is named, over the window of [`sensors`].
pub(super) fn id_pattern(ty: Option<&str>, raw: &str) -> CompiledTemporalQuery {
    let mut compiled = sensors();
    compiled.entities.selectors = vec![
        EntitySelector::builder()
            .type_expr(ty.map(|ty| TypeExpr::Type(iri(&format!("https://uri.etsi.org/ngsi-ld/default-context/{ty}")))))
            .id_pattern(Some(Pattern::parse(raw).expect("valid pattern")))
            .build(),
    ];
    compiled
}

/// The counted stage 1 of `compiled` over `branch` and `window`, under
/// [`sensor_names`].
pub(super) fn stage1_on(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, window: &PageWindow) -> Stage1Query {
    build_stage1_entity_ids(branch, compiled, window, CountPolicy::Emit, &sensor_names()).expect("valid temporal query")
}

/// The counted stage 1 of `compiled` over the live branch's recorded rows
/// alone, its first page of 20, under [`sensor_names`].
pub(super) fn recorded_stage1(compiled: &CompiledTemporalQuery) -> Stage1Query {
    stage1_on(&HistoryBranch::live_recorded(), compiled, &PageWindow::offset(20, 0))
}

/// The parts of a rendered gated page.
pub(super) struct PageBranches<'s> {
    /// The short walk's statement.
    pub(super) short: &'s str,
    /// The candidate read, without its bound.
    pub(super) candidates: &'s str,
    /// The branch cut from the candidates.
    pub(super) indexed: &'s str,
    /// The branch cut from the full walk.
    pub(super) walk: &'s str,
}

impl PageBranches<'_> {
    /// The parts of the gated page `sql`.
    pub(super) fn of(sql: &str) -> PageBranches<'_> {
        let after = |text: &'_ str, marker: &str| -> usize { text.find(marker).map_or_else(|| panic!("{marker} in {sql}"), |at| at + marker.len()) };
        let short_start = after(sql, r#""short_walk" AS  MATERIALIZED ("#);
        let short_end = sql
            .find(r#") , "candidates" AS  MATERIALIZED ("#)
            .unwrap_or_else(|| panic!("the candidates follow the short walk: {sql}"));
        let candidates_start = after(sql, r#""candidates" AS  MATERIALIZED ("#);
        let candidates_end = candidates_start + sql[candidates_start..].find(" LIMIT ").unwrap_or_else(|| panic!("a bounded read: {sql}"));
        let indexed_start = after(sql, r#"AS "answered""#);
        let indexed_end = sql.find(r#"AS "indexed""#).unwrap_or_else(|| panic!("the candidate branch: {sql}"));
        PageBranches {
            short: &sql[short_start..short_end],
            candidates: &sql[candidates_start..candidates_end],
            indexed: &sql[indexed_start..indexed_end],
            walk: &sql[indexed_end..],
        }
    }
}
