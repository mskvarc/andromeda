//! The queries, dictionaries and renderings the stage 1 tests share.

use crate::stage1::build_stage1_entity_ids;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity_selection::{EntitySelection, EntitySelector},
    page_window::PageWindow,
    temporal::{compiled::CompiledTemporalQuery, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
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
