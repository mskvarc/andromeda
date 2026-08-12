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

use crate::schema::{Ah, An, AttributeHistory, AttributeNames, CE_ALIAS, Ce, EntityIdColumn, S, TemporalAttributeStreams, TemporalEntities};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    entity_selection::EntitySelector,
    pagination::CountPolicy,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_query::{
    attribute_instance_correlation::CorrelatedEntityRow,
    bind::sea_values_to_bind_values,
    built::BuiltQuery,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::GeometrySource,
    operator::regex_match,
    q_predicate::{attribute_names::ResolvedAttributeNames, projection as q_filter, target::QFilterTarget},
    schema::{E, Entities},
    scope_predicate::{ScopeTarget, condition as scope_condition},
    time_column::history_column,
    type_predicate::{TypesColumn, condition as type_condition},
};
use iri_rs::IriBuf;
use sea_query::{Condition, Expr as SeaExpr, Func, JoinType, Order, PostgresQueryBuilder, Query, SelectStatement};

/// Build Stage 1 query: find entity IDs matching entity-level filters.
pub fn build_stage1_entity_ids(compiled: &CompiledTemporalQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, RenderError> {
    // Entity-level conditions, applied to `temporal_entities` under alias "e".
    let entity_conditions = build_entity_conditions_for_temporal(compiled, attribute_names)?;

    let temporal_match = temporal_match_subquery(compiled)?;

    // Build the main SELECT
    let mut select = Query::select();
    select
        .expr_as(SeaExpr::col((E, TemporalEntities::Id)), EntityIdColumn)
        .from_as(TemporalEntities::Table, E);

    select
        .cond_where(entity_conditions.clone()) // clone: count query applies the same entity predicate
        .cond_where(SeaExpr::exists(temporal_match))
        .order_by((E, TemporalEntities::Id), Order::Asc)
        .limit(compiled.pagination.limit as u64)
        .offset(compiled.pagination.offset as u64);

    // Build main query SQL
    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    // Build count query if requested
    let count_sql = if compiled.pagination.count == CountPolicy::Emit {
        let mut count_select = Query::select();
        count_select
            .expr(Func::count(SeaExpr::col((E, TemporalEntities::Id))))
            .from_as(TemporalEntities::Table, E)
            .cond_where(entity_conditions)
            .cond_where(SeaExpr::exists(temporal_match_subquery(compiled)?));
        let (count_sql_str, _) = count_select.build(PostgresQueryBuilder);
        Some(count_sql_str)
    } else {
        None
    };

    Ok(BuiltQuery { sql, count_sql, bind_values })
}

/// Build the correlated stream/history predicate shared by result and count.
fn temporal_match_subquery(compiled: &CompiledTemporalQuery) -> Result<SelectStatement, RenderError> {
    let temporal_conditions = build_temporal_stage1_conditions(compiled)?;
    let mut select = Query::select();
    select
        .expr(SeaExpr::val(1))
        .from_as(TemporalAttributeStreams::Table, S)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            SeaExpr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        )
        .cond_where(SeaExpr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((E, TemporalEntities::EntityKey)))
        .cond_where(temporal_conditions);

    if compiled.window.interval.is_bounded() {
        select.join_as(
            JoinType::Join,
            AttributeHistory::Table,
            Ah,
            SeaExpr::col((Ah, AttributeHistory::StreamId)).equals((S, TemporalAttributeStreams::Id)),
        );
    }
    Ok(select.to_owned())
}

/// Build entity-level conditions for temporal queries (using "e" alias for
/// the `temporal_entities` table).
fn build_entity_conditions_for_temporal(compiled: &CompiledTemporalQuery, attribute_names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Entity selectors, unioned (clause 5.2.11): `(sel1) OR (sel2) OR ...`,
    // each reached through the `e` alias stage 1 selects the temporal
    // catalogue under. Rendered only when every selector constrains — the
    // reading `EntitySelection::selectors_constrain` shares with the entity path.
    if compiled.entities.selectors_constrain() {
        let mut union = Condition::any();
        for selector in &compiled.entities.selectors {
            union = union.add(temporal_selector_condition(selector));
        }
        condition = condition.add(union);
    }

    if let Some(probe) = current_state_probe(compiled, attribute_names)? {
        condition = condition.add(SeaExpr::exists(probe));
    }

    Ok(condition)
}

/// The correlated `EXISTS` over `entities` carrying the current-state filters.
///
/// `q` (clause 4.9) reads the `current_attribute_values` projection and, for paths the
/// projection cannot answer, the entity's JSONB payload; a geo query (clause
/// 4.10) reads the default `location` columns or the instance rows of a named
/// GeoProperty. Both are statements about current state, and both name columns
/// that live on `entities` alone, so they are evaluated inside one probe
/// correlated to the temporal entity's URI. `None` when the query carries
/// neither filter, so an unfiltered temporal query never touches `entities`.
fn current_state_probe(compiled: &CompiledTemporalQuery, attribute_names: &ResolvedAttributeNames) -> Result<Option<SelectStatement>, RenderError> {
    if compiled.entities.q_expr.is_none() && compiled.entities.geo.is_none() {
        return Ok(None);
    }

    let mut current_conditions = Condition::all();

    if let Some(q_expr) = &compiled.entities.q_expr {
        current_conditions = current_conditions.add(q_filter::compile(
            q_expr,
            &QFilterTarget::correlated((Ce, Entities::EntityKey), (Ce, Entities::Payload)),
            attribute_names,
        )?);
    }

    if let Some(geo) = &compiled.entities.geo {
        current_conditions = current_conditions.add(geo_sql::condition(
            geo,
            GeometrySource::Entity {
                row: CorrelatedEntityRow::aliased(CE_ALIAS),
            },
            attribute_names,
        )?);
    }

    let mut probe = Query::select();
    probe
        .expr(SeaExpr::val(1))
        .from_as(Entities::Table, Ce)
        .cond_where(SeaExpr::col((Ce, Entities::Id)).equals((E, TemporalEntities::Id)))
        .cond_where(current_conditions);
    Ok(Some(probe.to_owned()))
}

/// Render one selector's identifier, pattern and type predicates against the
/// `e` alias stage 1 selects `temporal_entities` under, ANDed (clause 5.2.11).
///
/// The entity path renders the same three axes against `entities`; the columns
/// carry the same names on both relations, so the two cannot drift on what a
/// selector means.
fn temporal_selector_condition(selector: &EntitySelector) -> Condition {
    let mut condition = Condition::all();
    if !selector.ids.is_empty() {
        let ids: Vec<&str> = selector.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col((E, TemporalEntities::Id)).is_in(ids));
    }
    if let Some(pattern) = &selector.id_pattern {
        condition = condition.add(regex_match(
            SeaExpr::col((E, TemporalEntities::Id)).into(),
            SeaExpr::val(pattern.as_str()).into(),
        ));
    }
    if let Some(type_expr) = &selector.type_expr {
        condition = condition.add(type_condition(type_expr, TypesColumn::AliasedEntities));
    }
    condition
}

/// Build temporal-specific conditions for Stage 1 (attribute filters, datasetId, timerel, scopeQ).
fn build_temporal_stage1_conditions(compiled: &CompiledTemporalQuery) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // Attribute filter: an.name = ANY($N)
    if !compiled.instances.attr_filter.is_empty() {
        let names: Vec<&str> = compiled.instances.attr_filter.iter().map(IriBuf::as_str).collect();
        condition = condition.add(SeaExpr::col((An, AttributeNames::Name)).is_in(names));
    }

    // datasetId filter (clause 4.5.5): the default instance is stored as a
    // NULL `dataset_id`, so it is tested separately from the named ones.
    let dataset_column = SeaExpr::col((S, TemporalAttributeStreams::DatasetId));
    condition = match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => condition,
        DatasetFilter::DefaultOnly => condition.add(dataset_column.is_null()),
        DatasetFilter::NamedOnly(named) => condition.add(dataset_column.is_in(named.iter().map(ToString::to_string))),
        DatasetFilter::DefaultOrNamed(named) => condition.add(
            Condition::any()
                .add(SeaExpr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string)))
                .add(dataset_column.is_null()),
        ),
    };

    // Temporal filters (timerel + time_at), on the requested axis alone.
    let time_col = history_column(compiled.window.timeproperty);
    let bound =
        |sql: &'static str, instant: &chrono::DateTime<chrono::Utc>| SeaExpr::cust_with_exprs(sql, [SeaExpr::cust(time_col), SeaExpr::val(*instant).into()]);
    match &compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(bound("$1 < $2", instant)),
        TimeInterval::After(instant) => condition = condition.add(bound("$1 >= $2", instant)),
        TimeInterval::Between { from, to } => {
            let lower = bound("$1 >= $2", from);
            let upper = bound("$1 < $2", to);
            condition = condition.add(lower).add(upper);
        }
    }

    // Historical scope filtering via EXISTS subquery against scope_history,
    // which hangs off the temporal entity grain rather than off `entities`.
    if let Some(scope_q) = &compiled.entities.scope_q {
        let target = ScopeTarget::history((E, TemporalEntities::EntityKey), compiled.window.timeproperty, compiled.window.interval);
        condition = condition.add(scope_condition(scope_q, &target));
    }

    Ok(condition)
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
        scope::query::ScopeQuery,
        temporal::{instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, bind::BindValue};
    use chrono::{DateTime, Utc};

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    fn ids(raw: &[&str]) -> Vec<Uri> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    /// A GeoJSON point reference geometry, for tests that vary the relation
    /// rather than the geometry.
    fn point(coordinates: serde_json::Value) -> Geometry {
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

    #[test]
    fn build_entity_id_selection_with_type() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![
            EntitySelector::builder()
                .type_expr(Some(TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"))))
                .build(),
        ];
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(!result.sql.contains("SELECT DISTINCT"));
        assert!(result.sql.contains("EXISTS(SELECT"));
        assert!(result.sql.contains("\"e\".\"types\""));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    /// Clause 5.6.6 leaves the temporal evolution standing when the current
    /// entity is deleted, so the driving relation is the temporal catalogue and
    /// an unfiltered query names `entities` nowhere.
    #[test]
    fn the_statement_drives_from_the_temporal_catalogue_and_never_joins_entities() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("FROM \"temporal_entities\" AS \"e\""), "{}", result.sql);
        assert!(result.sql.contains("\"temporal_attribute_streams\""), "{}", result.sql);
        assert!(!result.sql.contains("\"entities\""), "{}", result.sql);
        assert!(!result.sql.contains("temporal_entity_attributes"), "{}", result.sql);
    }

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_correlates_on_the_temporal_entity_key() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("\"s\".\"temporal_entity_key\" = \"e\".\"entity_key\""), "{}", result.sql);
    }

    #[test]
    fn entity_id_selection_orders_by_id_for_stable_pagination() {
        // The driving temporal catalogue emits one row per id, and ordered
        // paging keeps clause 5.5.9 result pages stable.
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("ORDER BY \"e\".\"id\""), "{}", result.sql);
    }

    #[test]
    fn stage1_projects_the_entity_uri_as_entity_id() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("\"e\".\"id\" AS \"entity_id\""), "{}", result.sql);
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();

        assert!(
            !result.sql.contains("< $1"),
            "time placeholder must not collide with earlier id-filter placeholders"
        );
        assert!(!result.sql.contains("::timestamptz"));
        assert!(result.bind_values.iter().any(|bind| matches!(bind, BindValue::Timestamp(_))));
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
            let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
            assert!(result.sql.contains(column), "{prop}: {}", result.sql);
            assert!(!result.sql.contains("COALESCE"), "{prop}: {}", result.sql);
            assert!(!result.sql.contains("recorded_at"), "{prop}: {}", result.sql);
        }
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let unbounded = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(!unbounded.sql.contains("attribute_history"), "{}", unbounded.sql);

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let bounded = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(bounded.sql.contains("\"ah\".\"stream_id\" = \"s\".\"id\""), "{}", bounded.sql);
    }

    #[test]
    fn stage1_count_reuses_the_result_predicate_bind_order() {
        let mut compiled = minimal_compiled();
        compiled.pagination.count = CountPolicy::Emit;
        compiled.entities.selectors = vec![EntitySelector::builder().ids(ids(&["urn:ngsi-ld:Vehicle:1"])).build()];
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        let count_sql = result.count_sql.expect("count SQL is requested");

        assert!(count_sql.contains("EXISTS(SELECT"), "{count_sql}");
        assert!(count_sql.contains("$1"), "{count_sql}");
        assert!(count_sql.contains("$2"), "{count_sql}");
        assert!(result.bind_values.iter().any(|bind| matches!(bind, BindValue::Timestamp(_))));
    }

    #[test]
    fn stage1_includes_attribute_names_join() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn stage1_includes_scope_q_filter() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/Gardens").expect("valid scope query"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("scope_history"), "SQL should reference scope_history table");
        assert!(result.sql.contains("lquery"), "SQL should contain lquery cast");
    }

    /// Scope history hangs off the temporal entity, so the probe correlates on
    /// the surrogate key the rebuilt relation carries.
    #[test]
    fn scope_history_correlates_on_the_temporal_entity_key() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("\"e\".\"entity_key\""), "{}", result.sql);
    }

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes");
        assert!(result.sql.contains("OR"), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn temporal_scope_q_between() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Between {
            from: instant("2024-01-01T00:00:00Z"),
            to: instant("2024-06-01T00:00:00Z"),
        };
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains(">="), "Between should bound the scope probe from below");
        assert!(result.sql.contains("<"), "Between should bound the scope probe from above");
    }

    #[test]
    fn temporal_scope_q_compound() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        // /A;/B -> AND, ,/C -> OR
        assert!(result.sql.contains("AND"), "Semicolon scopes produce AND");
        assert!(result.sql.contains("OR"), "Comma scopes produce OR");
        // Each scope term has its own EXISTS subquery, inside the stream
        // catalogue EXISTS predicate.
        let exists_count = result.sql.matches("EXISTS").count();
        pretty_assertions::assert_eq!(exists_count, 4, "The catalogue predicate wraps three scope predicates");
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
        let names = ResolvedAttributeNames::from_pairs([("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameId::from(7))]);
        let result = build_stage1_entity_ids(&compiled, &names).unwrap();
        assert!(result.sql.contains("FROM \"entities\" AS \"ce\""), "{}", result.sql);
        assert!(result.sql.contains("\"ce\".\"id\" = \"e\".\"id\""), "{}", result.sql);
        assert!(result.sql.contains("\"current_attribute_values\""), "{}", result.sql);
        assert!(result.sql.contains("\"cav\".\"entity_key\" = \"ce\".\"entity_key\""), "{}", result.sql);
        assert!(!result.sql.contains("\"cav\".\"entity_id\""), "{}", result.sql);
        assert!(!result.sql.contains("jsonb_path_query_first"), "{}", result.sql);
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/never-written\">50").unwrap());
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("FALSE"), "{}", result.sql);
    }

    /// The jsonpath fallback reads the payload, which only the current-state row
    /// carries, so it is rendered against the probe's alias.
    #[test]
    fn a_q_predicate_on_a_compound_value_reads_the_current_state_payload() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("address[city]==\"Berlin\"").unwrap());
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("\"ce\".\"payload\""), "{}", result.sql);
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(!result.sql.contains("scope_history"), "No scope_history reference without scopeQ");
    }

    /// Clause 4.10 applies to a temporal query exactly as it does to a current-
    /// state one: a named GeoProperty is read from its `current_attribute_values`
    /// instance rows, correlated through the current-state probe.
    #[test]
    fn a_temporal_geo_query_on_a_named_geoproperty_reads_its_attribute_instance_rows() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/customGeo".parse().expect("valid attribute name"),
        });
        let names = ResolvedAttributeNames::from_pairs([(
            "https://example.com/customGeo".parse().expect("valid attribute name"),
            AttributeNameId::from(42_i32),
        )]);
        let result = build_stage1_entity_ids(&compiled, &names).unwrap();
        assert!(result.sql.contains("EXISTS (SELECT 1 FROM \"current_attribute_values\""), "{}", result.sql);
        assert!(result.sql.contains("\"attribute_name_id\" = 42"), "{}", result.sql);
        assert!(
            result.sql.contains("\"geo_instance\".\"entity_key\" = \"ce\".\"entity_key\""),
            "correlated through the current-state probe: {}",
            result.sql
        );
    }

    /// The IRI is reached by dictionary key, so an apostrophe in one never
    /// enters the statement text.
    #[test]
    fn a_temporal_geoproperty_iri_never_reaches_the_statement_text() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty: "https://example.com/o'brien".parse().expect("valid attribute name"),
        });
        let names = ResolvedAttributeNames::from_pairs([(
            "https://example.com/o'brien".parse().expect("valid attribute name"),
            AttributeNameId::from(9_i32),
        )]);
        let result = build_stage1_entity_ids(&compiled, &names).unwrap();
        assert!(!result.sql.contains("brien"), "{}", result.sql);
        // Every literal contributes a balanced pair of quotes, and a doubled
        // apostrophe contributes two, so the total is even unless a literal was
        // terminated early.
        assert_eq!(result.sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {}", result.sql);
    }
}
