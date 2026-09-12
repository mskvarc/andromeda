//! Stage 1 of a temporal query: select the entity ids that match every filter.
//!
//! The statement drives from `temporal_entities`, never from `entities`. Core
//! Entity deletion (clause 5.6.6) removes the current-state row and leaves the
//! temporal evolution standing, so an entity's history has to stay selectable
//! after it; the entity URI and the entity types are carried on the temporal
//! catalogue for exactly that reason. Current-state filters - `q` and the geo
//! query - reach `entities` through a correlated `EXISTS` instead, which is
//! what makes "no current state" mean "matches no current-state filter" rather
//! than "history disappears".

use crate::{
    alias::{Ah, Ahg, An, Ang, Ce, Cs, EntityIdColumn, S, Sg},
    axis::axis_column,
};
use andromeda_context_space::context_space_key::ContextSpaceKey;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    entity_selection::{ConsideredEntities, EntitySelector},
    pagination::CountPolicy,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_query::{
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_ordering::{order_by_entity_identifier, ordering_collation},
    entity_row_refs::{DocumentsRef, EntityRowRefs},
    error::RenderError,
    geo_predicate::relation_expression,
    q_predicate::projection as q_filter,
    scope_predicate::{ScopeTarget, condition as scope_condition},
    type_predicate::condition as type_condition,
};
use andromeda_postgresql_schema::{
    current::{E, Entities},
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::{false_literal, int_literal},
    operator::regex_match,
    pg_type::PostgresType,
};
use iri_rs::IriBuf;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Func, JoinType, Order, SelectStatement};

/// The Stage 1 statement and its optional count, each a statement tree the store
/// binds and executes on its own.
///
/// The result and the count carry the same entity predicate but different tails
/// (the result paginates and orders, the count neither), so each owns its own
/// placeholder sequence rather than sharing one, which is what a paginated
/// result and an unpaginated count cannot do.
pub struct Stage1Query {
    /// The paginated statement that selects the matching entity ids.
    pub select: SelectStatement,
    /// The unpaginated `COUNT`, present when clause 5.5.9 asks for a total.
    pub count: Option<SelectStatement>,
}

/// Build Stage 1 query: find entity IDs matching entity-level filters.
///
/// # Errors
///
/// Returns [`RenderError::Query`] when the current-state filters cannot be
/// rendered: a `q` term (clause 4.9) naming an operator the store cannot apply
/// to the target path, or a geo query (clause 4.10) whose relation and geometry
/// combination clause 4.10 does not admit. Returns
/// [`RenderError::ReferenceGeometryEncoding`] when the geo query's reference
/// geometry cannot be encoded as `PostGIS` EWKB.
pub fn build_stage1_entity_ids(context_space_key: ContextSpaceKey, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Result<Stage1Query, RenderError> {
    // Entity-level conditions, applied to `temporal_entities` under alias "e".
    let entity_conditions = build_entity_conditions_for_temporal(context_space_key, compiled, names)?;

    let mut select = SelectStatement::new();
    select
        .expr_as(SeaExpr::col((E, TemporalEntities::Id)), EntityIdColumn)
        .from_as(TemporalEntities::Table, E)
        .cond_where(entity_conditions.clone()) // clone: the count query applies the same entity predicate
        .cond_where(SeaExpr::exists(temporal_match_subquery(context_space_key, compiled)));

    // Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    // the JSON-LD array returned shall be ordered according to the member names
    // specified." Only `id` can be named (clause 5.7.4.3), and the collation is
    // written out rather than left to the column: `temporal_entities.id` is
    // declared `COLLATE "C"`, so an unstated comparison is byte-wise and the
    // collation Table 6.18.3.2-1 says the payload "shall be ordered according
    // to" would have no effect at all.
    if let Some(ordering) = compiled.ordering.as_ref() {
        order_by_entity_identifier(
            &mut select,
            SeaExpr::col((E, TemporalEntities::Id)),
            ordering.direction,
            &ordering_collation(ordering.collation.as_ref()),
        );
    }

    // Clause 5.5.9 asks for a consistent order across pages whether or not one
    // was requested: `LIMIT`/`OFFSET` over an otherwise unordered result can
    // repeat and skip rows between pages.
    select
        .order_by((E, TemporalEntities::Id), Order::Asc)
        .limit(u64::from(compiled.pagination.limit))
        .offset(u64::from(compiled.pagination.offset));

    let count = if compiled.pagination.count == CountPolicy::Emit {
        let mut count_select = SelectStatement::new();
        count_select
            .expr(Func::count(SeaExpr::col((E, TemporalEntities::Id))))
            .from_as(TemporalEntities::Table, E)
            .cond_where(entity_conditions)
            .cond_where(SeaExpr::exists(temporal_match_subquery(context_space_key, compiled)));
        Some(count_select.take())
    } else {
        None
    };

    Ok(Stage1Query { select: select.take(), count })
}

/// Build the correlated stream/history predicate shared by result and count.
fn temporal_match_subquery(context_space_key: ContextSpaceKey, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let temporal_conditions = build_temporal_stage1_conditions(compiled);
    let mut select = SelectStatement::new();
    select
        .expr(int_literal(1))
        .from_as(TemporalAttributeStreams::Table, S)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            SeaExpr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        )
        .cond_where(SeaExpr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((E, TemporalEntities::EntityKey)))
        .cond_where(SeaExpr::col((S, TemporalAttributeStreams::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .cond_where(temporal_conditions);

    if compiled.window.interval.is_bounded() {
        select.join_as(
            JoinType::Join,
            AttributeHistory::Table,
            Ah,
            SeaExpr::col((Ah, AttributeHistory::StreamKey)).equals((S, TemporalAttributeStreams::StreamKey)),
        );
        select.cond_where(SeaExpr::col((Ah, AttributeHistory::ContextSpaceKey)).eq(context_space_key.as_i64()));
    }
    select.take()
}

/// Build entity-level conditions for temporal queries (using "e" alias for
/// the `temporal_entities` table).
fn build_entity_conditions_for_temporal(
    context_space_key: ContextSpaceKey,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
) -> Result<Condition, RenderError> {
    let mut condition = Condition::all().add(SeaExpr::col((E, TemporalEntities::ContextSpaceKey)).eq(context_space_key.as_i64()));

    // Entity selectors, unioned (clause 5.2.33): `(sel1) OR (sel2) OR ...`,
    // each reached through the `e` alias stage 1 selects the temporal
    // catalogue under. Rendered only when every selector constrains — the
    // reading `EntitySelection::selectors_constrain` shares with the entity path.
    if compiled.entities.selectors_constrain() {
        let mut union = Condition::any();
        for selector in &compiled.entities.selectors {
            union = union.add(temporal_selector_condition(selector, &names.types));
        }
        condition = condition.add(union);
    }

    // Clause 5.5.14: "An EntityMap fixes the Entities to be considered." A
    // request narrowed to no entity at all cannot say so through a selector's
    // identifier list, where an empty list reads as no identifier constraint --
    // so it is conjoined here, where nothing can widen it back.
    match compiled.entities.considered {
        ConsideredEntities::Selected => {}
        ConsideredEntities::NoEntity => condition = condition.add(false_literal()),
    }

    if let Some(probe) = current_state_probe(context_space_key, compiled, names) {
        condition = condition.add(SeaExpr::exists(probe));
    }

    if let Some(probe) = temporal_geo_existential(context_space_key, compiled)? {
        condition = condition.add(SeaExpr::exists(probe));
    }

    Ok(condition)
}

/// The correlated `EXISTS` over the current-state relations carrying the `q`
/// filter (clause 4.9).
///
/// `q` reads the `current_attribute_values` projection and, for paths the
/// projection cannot answer, the entity's JSONB payload, which the probe reads
/// by key lookup rather than by joining `entity_documents`. It is a statement
/// about current state, evaluated inside a probe correlated to the temporal
/// entity's URI. `None` when the query carries no `q`, so a temporal query with
/// no `q` never touches current state.
///
/// A geo query (clause 4.10) is deliberately *not* here: clause 5.7.4.4 step S3
/// checks its restriction against the `GeoProperty` instances "within the
/// interval defined by the temporal query", which are the `attribute_history`
/// rows, not current state. [`temporal_geo_existential`] carries it.
fn current_state_probe(context_space_key: ContextSpaceKey, compiled: &CompiledTemporalQuery, names: &QueryNames) -> Option<SelectStatement> {
    let q_expr = compiled.entities.q_expr.as_ref()?;

    let rows = EntityRowRefs::aliased(Ce, Cs, DocumentsRef::ByKeyLookup);

    let mut probe = SelectStatement::new();
    probe
        .expr(int_literal(1))
        .from_as(Entities::Table, Ce)
        .cond_where(SeaExpr::col((Ce, Entities::Id)).equals((E, TemporalEntities::Id)))
        .cond_where(SeaExpr::col((Ce, Entities::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .cond_where(q_filter::compile(q_expr, &rows, names));
    Some(probe.take())
}

/// The correlated `EXISTS` over the `attribute_history` rows a temporal geo
/// query (clause 4.10) filters, restricted to the temporal window.
///
/// Clause 5.7.4.4 step S3: the geospatial restriction "shall be checked against
/// the `GeoProperty` instances that are within the interval defined by the
/// temporal query". Every instance of the target `GeoProperty` - the default
/// `location` and any named one alike - is an `attribute_history` row carrying a
/// `value_geo`, so the probe reaches them uniformly through the stream rather
/// than through the single hoisted current-state `location` column, which holds
/// only one instant and answers a different question. Clause 4.10 makes one
/// in-window instance meeting the restriction enough, which is exactly this
/// existential. `None` when the query carries no geo restriction.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] or [`RenderError::Query`]
/// exactly as the shared relation rendering does.
fn temporal_geo_existential(context_space_key: ContextSpaceKey, compiled: &CompiledTemporalQuery) -> Result<Option<SelectStatement>, RenderError> {
    let Some(geo) = &compiled.entities.geo else {
        return Ok(None);
    };

    let planar = SeaExpr::col((Ahg, AttributeHistory::ValueGeo));
    let geographic = cast(SeaExpr::col((Ahg, AttributeHistory::ValueGeo)), PostgresType::Geography);
    let spatial = relation_expression(&geo.georel, planar, geographic, &geo.geometry)?;

    let mut probe = SelectStatement::new();
    probe
        .expr(int_literal(1))
        .from_as(TemporalAttributeStreams::Table, Sg)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            Ang,
            SeaExpr::col((Ang, AttributeNames::Id)).equals((Sg, TemporalAttributeStreams::AttributeNameId)),
        )
        .join_as(
            JoinType::Join,
            AttributeHistory::Table,
            Ahg,
            SeaExpr::col((Ahg, AttributeHistory::StreamKey)).equals((Sg, TemporalAttributeStreams::StreamKey)),
        )
        .cond_where(SeaExpr::col((Sg, TemporalAttributeStreams::TemporalEntityKey)).equals((E, TemporalEntities::EntityKey)))
        .cond_where(SeaExpr::col((Sg, TemporalAttributeStreams::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .cond_where(SeaExpr::col((Ahg, AttributeHistory::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .cond_where(SeaExpr::col((Ang, AttributeNames::Name)).eq(geo.geoproperty.as_str()))
        .cond_where(SeaExpr::col((Ahg, AttributeHistory::ValueGeo)).is_not_null())
        .cond_where(geo_window_condition(compiled))
        .cond_where(spatial);
    Ok(Some(probe.take()))
}

/// The temporal window as a predicate on the geo existential's own
/// `attribute_history` axis column.
///
/// The same interval the stream match applies (clause 5.2.21), restated against
/// the `ahg` alias so a `GeoProperty` instance counts only when it falls within
/// the window (clause 5.7.4.4 S3). The axis is the query's `timeproperty`,
/// defaulting to `observedAt` (clause 4.11).
fn geo_window_condition(compiled: &CompiledTemporalQuery) -> Condition {
    let axis = axis_column(compiled.window.timeproperty);
    let mut condition = Condition::all();
    match &compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(SeaExpr::col((Ahg, axis)).lt(*instant)),
        TimeInterval::After(instant) => condition = condition.add(SeaExpr::col((Ahg, axis)).gte(*instant)),
        TimeInterval::Between { from, to } => {
            condition = condition.add(SeaExpr::col((Ahg, axis)).gte(*from)).add(SeaExpr::col((Ahg, axis)).lt(*to));
        }
    }
    condition
}

/// Render one selector's identifier, pattern and type predicates against the
/// `e` alias stage 1 selects `temporal_entities` under, `ANDed` (clause 5.2.33).
///
/// The entity path renders the same three axes against `entities`; the columns
/// carry the same names on both relations, so the two cannot drift on what a
/// selector means.
fn temporal_selector_condition(selector: &EntitySelector, types: &ResolvedNames<EntityTypeName>) -> Condition {
    let mut condition = Condition::all();
    if !selector.ids.is_empty() {
        let ids: Vec<&str> = selector.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col((E, TemporalEntities::Id)).is_in(ids));
    }
    if let Some(pattern) = &selector.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col((E, TemporalEntities::Id)), SeaExpr::val(pattern.as_str())));
    }
    if let Some(type_expr) = &selector.type_expr {
        condition = condition.add(type_condition(type_expr, &SeaExpr::col((E, TemporalEntities::TypeIds)), types));
    }
    condition
}

/// Build temporal-specific conditions for Stage 1 (attribute filters, `datasetId`, timerel, `scopeQ`).
fn build_temporal_stage1_conditions(compiled: &CompiledTemporalQuery) -> Condition {
    let mut condition = Condition::all();

    // Attribute filter: an.name IN (...).
    if !compiled.instances.attr_filter.is_empty() {
        let names: Vec<&str> = compiled.instances.attr_filter.iter().map(IriBuf::as_str).collect();
        condition = condition.add(SeaExpr::col((An, AttributeNames::Name)).is_in(names));
    }

    // datasetId filter (clause 4.5.5): the default instance is stored as a
    // NULL `dataset_id`, so it is tested separately from the named ones.
    condition = match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => condition,
        DatasetFilter::DefaultOnly => condition.add(SeaExpr::col((S, TemporalAttributeStreams::DatasetId)).is_null()),
        DatasetFilter::NamedOnly(named) => condition.add(SeaExpr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string))),
        DatasetFilter::DefaultOrNamed(named) => condition.add(
            Condition::any()
                .add(SeaExpr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string)))
                .add(SeaExpr::col((S, TemporalAttributeStreams::DatasetId)).is_null()),
        ),
    };

    // Temporal filters (timerel + time_at), on the requested axis alone.
    let axis = axis_column(compiled.window.timeproperty);
    match &compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(SeaExpr::col((Ah, axis)).lt(*instant)),
        TimeInterval::After(instant) => condition = condition.add(SeaExpr::col((Ah, axis)).gte(*instant)),
        TimeInterval::Between { from, to } => {
            condition = condition.add(SeaExpr::col((Ah, axis)).gte(*from)).add(SeaExpr::col((Ah, axis)).lt(*to));
        }
    }

    // Historical scope filtering via EXISTS subquery against scope_history,
    // which hangs off the temporal entity grain rather than off `entities`.
    if let Some(scope_q) = &compiled.entities.scope_q {
        let target = ScopeTarget::history((E, TemporalEntities::EntityKey), compiled.window.timeproperty, compiled.window.interval);
        condition = condition.add(scope_condition(scope_q, &target));
    }

    condition
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
    use andromeda_ngsi_ld_query::{
        entity_selection::{EntitySelection, EntitySelector},
        filter::parser,
        geo::query::GeoQuery,
        ordering::{collation::IcuCollation, sort_direction::SortDirection},
        scope::query::ScopeQuery,
        temporal::{instance_selection::InstanceSelection, ordering::TemporalEntityOrdering, representation::Representation, window::TemporalWindow},
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, entity_type_id::EntityTypeId};
    use chrono::{DateTime, Utc};
    use sea_query::{PostgresQueryBuilder, Value, Values};

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// The type dictionary with the one type the selector tests name.
    fn vehicle_is_known() -> QueryNames {
        QueryNames {
            attributes: ResolvedNames::default(),
            types: ResolvedNames::from_pairs([(
                "https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("valid type name"),
                EntityTypeId::from(3),
            )]),
        }
    }

    /// A `GeoJSON` point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses")
    }

    /// A query with every filter absent, for tests that vary one part at a time.
    fn minimal_compiled() -> CompiledTemporalQuery {
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
    fn built(compiled: &CompiledTemporalQuery, names: &QueryNames) -> (String, Values) {
        let context_space_key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        build_stage1_entity_ids(context_space_key, compiled, names)
            .expect("valid temporal query")
            .select
            .build(PostgresQueryBuilder)
    }

    /// The same for the count statement, panicking when no count was requested.
    fn built_count(compiled: &CompiledTemporalQuery, names: &QueryNames) -> String {
        let context_space_key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        build_stage1_entity_ids(context_space_key, compiled, names)
            .expect("valid temporal query")
            .count
            .expect("count requested")
            .build(PostgresQueryBuilder)
            .0
    }

    fn has_timestamp(values: &Values) -> bool {
        values.0.iter().any(|value| matches!(value, Value::ChronoDateTimeUtc(_)))
    }

    #[test]
    fn build_entity_id_selection_with_type() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"))))
                .build(),
        ];
        let (sql, _) = built(&compiled, &vehicle_is_known());
        assert!(!sql.contains("SELECT DISTINCT"), "{sql}");
        assert!(sql.contains("EXISTS(SELECT"), "{sql}");
        assert!(sql.contains("\"e\".\"type_ids\" @> (ARRAY[3])"), "{sql}");
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the temporal catalogue and
    /// an unfiltered query names `entities` nowhere.
    #[test]
    fn the_statement_drives_from_the_temporal_catalogue_and_never_joins_entities() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("FROM \"temporal_entities\" AS \"e\""), "{sql}");
        assert!(sql.contains("\"temporal_attribute_streams\""), "{sql}");
        assert!(!sql.contains("\"entities\""), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_correlates_on_the_temporal_entity_key() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"s\".\"temporal_entity_key\" = \"e\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn entity_id_selection_orders_by_id_for_stable_pagination() {
        // The driving temporal catalogue emits one row per id, and ordered
        // paging keeps clause 5.5.9 result pages stable.
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("ORDER BY \"e\".\"id\""), "{sql}");
    }

    #[test]
    fn stage1_projects_the_entity_uri_as_entity_id() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"e\".\"id\" AS \"entity_id\""), "{sql}");
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (sql, values) = built(&compiled, &QueryNames::default());

        assert!(
            !sql.contains("< $1"),
            "time placeholder must not collide with earlier id-filter placeholders: {sql}"
        );
        assert!(!sql.contains("::timestamptz"), "{sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    /// Clause 4.11 defaults the selector to `observedAt` and clause 5.2.21
    /// admits only the four semantic axes, so the window filters the requested
    /// column and nothing stands in for a missing value on it.
    #[test]
    fn each_time_property_filters_its_own_column_with_no_fallback() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\""),
            (TimeProp::Created, "\"ah\".\"created_at\""),
            (TimeProp::Modified, "\"ah\".\"modified_at\""),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\""),
        ];
        for (prop, column) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
            let (sql, _) = built(&compiled, &QueryNames::default());
            assert!(sql.contains(column), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
            assert!(!sql.contains("recorded_at"), "{prop}: {sql}");
        }
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let (unbounded, _) = built(&compiled, &QueryNames::default());
        assert!(!unbounded.contains("attribute_history"), "{unbounded}");

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (bounded, _) = built(&compiled, &QueryNames::default());
        assert!(bounded.contains("\"ah\".\"stream_key\" = \"s\".\"stream_key\""), "{bounded}");
    }

    #[test]
    fn stage1_count_reuses_the_result_predicate_bind_order() {
        let mut compiled = minimal_compiled();
        compiled.pagination.count = CountPolicy::Emit;
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let (_, values) = built(&compiled, &QueryNames::default());
        let count_sql = built_count(&compiled, &QueryNames::default());

        assert!(count_sql.contains("EXISTS(SELECT"), "{count_sql}");
        assert!(count_sql.contains("$1"), "{count_sql}");
        assert!(count_sql.contains("$2"), "{count_sql}");
        assert!(has_timestamp(&values), "the window instant is bound: {values:?}");
    }

    #[test]
    fn stage1_includes_attribute_names_join() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/Gardens").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("scope_history"), "SQL should reference scope_history table: {sql}");
        assert!(sql.contains("lquery"), "SQL should contain lquery cast: {sql}");
    }

    /// Scope history hangs off the temporal entity, so the probe correlates on
    /// the surrogate key the rebuilt relation carries.
    #[test]
    fn scope_history_correlates_on_the_temporal_entity_key() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"e\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes: {sql}");
        assert!(sql.contains("OR"), "SQL should contain OR for comma-separated scopes: {sql}");
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(">="), "Between should bound the scope probe from below: {sql}");
        assert!(sql.contains('<'), "Between should bound the scope probe from above: {sql}");
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        // /A;/B -> AND, ,/C -> OR
        assert!(sql.contains("AND"), "Semicolon scopes produce AND: {sql}");
        assert!(sql.contains("OR"), "Comma scopes produce OR: {sql}");
        // Each scope term has its own EXISTS subquery, inside the stream
        // catalogue EXISTS predicate.
        let exists_count = sql.matches("EXISTS").count();
        pretty_assertions::assert_eq!(exists_count, 4, "The catalogue predicate wraps three scope predicates: {sql}");
    }

    /// Clause 4.9: a `q` expression is a statement about current state, so it is
    /// evaluated inside a probe correlated to the temporal entity's URI rather
    /// than by joining `entities` into the driving statement. Inside that probe
    /// the projection is reached by the current-state surrogate key, which only
    /// the `entities` row the probe already selected can supply.
    #[test]
    fn stage1_q_filter_probes_current_state_through_the_entity_surrogate_key() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">50").unwrap());
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameId::from(7))]),
            types: ResolvedNames::default(),
        };
        let (sql, _) = built(&compiled, &names);
        assert!(sql.contains("FROM \"entities\" AS \"ce\""), "{sql}");
        assert!(sql.contains("\"ce\".\"id\" = \"e\".\"id\""), "{sql}");
        assert!(sql.contains("\"current_attribute_values\""), "{sql}");
        assert!(sql.contains("\"cav\".\"entity_key\" = \"ce\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("\"cav\".\"entity_id\""), "{sql}");
        assert!(!sql.contains("jsonb_path_query_first"), "{sql}");
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/never-written\">50").unwrap());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("FALSE"), "{sql}");
    }

    /// The jsonpath fallback reads the payload, which only current state
    /// carries, so it is read by key lookup correlated to the probe's alias
    /// rather than by joining the document row into every probe.
    #[test]
    fn a_q_predicate_on_a_compound_value_reads_the_current_state_payload() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("address[city]==\"Berlin\"").unwrap());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(
            sql.contains(r#"(SELECT "payload" FROM "entity_documents" WHERE "entity_key" = "ce"."entity_key")"#),
            "{sql}"
        );
        assert!(!sql.contains("entity_search"), "a q filter alone joins no search row: {sql}");
    }

    /// Clause 5.7.4.4 S3: the geospatial restriction is checked against the
    /// `GeoProperty` instances "within the interval defined by the temporal
    /// query", which are `attribute_history` rows -- not the single hoisted
    /// current-state `location` column, which holds one instant and answers a
    /// different question. The default `location` reaches its history rows
    /// through the stream just as any named `GeoProperty` does.
    #[test]
    fn a_default_location_geo_query_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point"),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"FROM "temporal_attribute_streams" AS "sg""#), "{sql}");
        assert!(sql.contains(r#"JOIN "attribute_history" AS "ahg""#), "{sql}");
        assert!(sql.contains(r#""ahg"."value_geo""#), "{sql}");
        assert!(
            sql.contains(r#""sg"."temporal_entity_key" = "e"."entity_key""#),
            "correlated to the temporal entity: {sql}"
        );
        assert!(!sql.contains("entity_search"), "the geo restriction no longer reads current state: {sql}");
    }

    /// Clause 5.7.4.4 S3 restricts the geo instances to the temporal window, so
    /// the geo existential restates the window predicate on its own
    /// `attribute_history` axis rather than trusting the stream match alone.
    #[test]
    fn a_temporal_geo_query_restricts_the_geo_instances_to_the_window() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2020-07-01T12:05:00Z"));
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(2000.0),
                min_distance: None,
            },
            geometry: point(&serde_json::json!([-8.503, 41.202])),
            geoproperty: "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#""ahg"."observed_at""#), "the window is applied to the geo instances: {sql}");
        assert!(sql.contains("ST_DWithin"), "near renders a distance relation on the geo instances: {sql}");
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("scope_history"), "No scope_history reference without scopeQ: {sql}");
    }

    /// Clause 5.7.4.4 S3 applies to a named `GeoProperty` exactly as to the
    /// default `location`: its instances are the `attribute_history` rows of the
    /// stream carrying that name, restricted to the window, so a named property
    /// reaches the same history rows the default one does rather than the
    /// current-state projection.
    #[test]
    fn a_temporal_geo_query_on_a_named_geoproperty_reads_the_windowed_history_instances() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"FROM "temporal_attribute_streams" AS "sg""#), "{sql}");
        assert!(
            sql.contains(r#"JOIN "attribute_names" AS "ang""#),
            "the target GeoProperty is named through the dictionary: {sql}"
        );
        assert!(sql.contains(r#""ahg"."value_geo""#), "{sql}");
        assert!(
            !sql.contains("current_attribute_values"),
            "a temporal geo query reads history, not the current-state projection: {sql}"
        );
    }

    /// The IRI is reached by dictionary key, so an apostrophe in one never
    /// enters the statement text.
    #[test]
    fn a_temporal_geoproperty_iri_never_reaches_the_statement_text() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(&serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
        });
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(
                "https://example.com/o'brien".parse().expect("valid attribute name"),
                AttributeNameId::from(9_i32),
            )]),
            types: ResolvedNames::default(),
        };
        let (sql, _) = built(&compiled, &names);
        assert!(!sql.contains("brien"), "{sql}");
        // Every literal contributes a balanced pair of quotes, so the total is
        // even unless a literal was terminated early.
        assert_eq!(sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {sql}");
    }

    /// Clause 5.7.4.5: "If Entity ordering is specified (see clause 4.23), then
    /// the JSON-LD array returned shall be ordered according to the member
    /// names specified", and Table 6.18.3.2-1 makes the collation the
    /// comparison the payload "shall be ordered according to". The identifier
    /// column is `COLLATE "C"`, so the requested collation has to be written
    /// into the key or the ordering is byte-wise whatever was asked for.
    #[test]
    fn a_requested_ordering_collates_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering {
            direction: SortDirection::Ascending,
            collation: Some(IcuCollation::parse("de-u-co-phonebk").expect("valid collation")),
        });
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "de-u-co-phonebk" ASC NULLS LAST"#), "{sql}");
    }

    /// Clause 4.23.1 makes ICU "root" (`und-x-icu`) the default collation
    /// order, so an ordering that named none still names one rather than
    /// falling back to the column's byte-wise `COLLATE "C"`.
    #[test]
    fn an_ordering_without_a_collation_compares_under_the_icu_root_order() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" ASC NULLS LAST"#), "{sql}");
    }

    /// Table 6.18.3.2-1 gives `desc` as one of the two sorting styles.
    #[test]
    fn a_descending_ordering_reverses_the_identifier_key() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains(r#"("e"."id") COLLATE "und-x-icu" DESC NULLS LAST"#), "{sql}");
    }

    /// The clause 5.5.9 tiebreaker survives an explicit ordering, so a page
    /// boundary never falls inside a group the requested key left tied.
    #[test]
    fn the_stable_identifier_tiebreaker_follows_a_requested_ordering() {
        let mut compiled = minimal_compiled();
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Descending).build());
        let (sql, _) = built(&compiled, &QueryNames::default());
        let collated = sql.find("COLLATE").expect("the requested key is written");
        let tiebreaker = sql.rfind(r#""e"."id" ASC"#).expect("the stable key is written");
        assert!(collated < tiebreaker, "{sql}");
    }

    /// Table 6.18.3.2-1 makes `collation` "part of Entity ordering", so a query
    /// that ordered nothing keeps the store's own stable order and pays for no
    /// collation at all.
    #[test]
    fn a_query_that_requested_no_ordering_keeps_the_stable_identifier_order() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(!sql.contains("COLLATE"), "{sql}");
        assert!(sql.contains(r#"ORDER BY "e"."id" ASC"#), "{sql}");
    }

    /// The count answers "how many", which no ordering changes, so clause 5.5.9
    /// gets its total without a sort the planner would have to perform.
    #[test]
    fn the_count_statement_carries_no_ordering() {
        let mut compiled = minimal_compiled();
        compiled.pagination.count = CountPolicy::Emit;
        compiled.ordering = Some(TemporalEntityOrdering::builder().direction(SortDirection::Ascending).build());
        let sql = built_count(&compiled, &QueryNames::default());
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }
}
