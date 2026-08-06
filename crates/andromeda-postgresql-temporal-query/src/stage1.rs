//! Stage 1 of a temporal query: select the entity ids that match every filter.

use crate::{
    schema::{Ah, An, AttributeHistory, AttributeNames, Tea, TemporalEntityAttributes},
    time_column::history_column,
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    pagination::CountPolicy,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_query::{
    bind::sea_values_to_bind_values,
    built::BuiltQuery,
    error::RenderError,
    geo_predicate as geo_sql,
    geo_predicate::{GeometrySource, NullGeometryPolicy},
    operator::regex_match,
    q_predicate::{attribute_names::ResolvedAttributeNames, projection as q_filter, target::QFilterTarget},
    schema::{E, Entities},
    scope_predicate::{ScopeTarget, condition as scope_condition},
    type_predicate::{TypesColumn, condition as type_condition},
};
use iri_rs::IriBuf;
use sea_query::{Alias, Condition, Expr as SeaExpr, Func, JoinType, Order, PostgresQueryBuilder, Query};
use urn_rs::Urn;

/// Build Stage 1 query: find entity IDs matching entity-level filters.
pub fn build_stage1_entity_ids(compiled: &CompiledTemporalQuery, attribute_names: &ResolvedAttributeNames) -> Result<BuiltQuery<'static>, RenderError> {
    // Build entity-level conditions (applied to entities table via alias "e")
    let entity_conditions = build_entity_conditions_for_temporal(compiled, attribute_names)?;

    // Build temporal-specific conditions
    let temporal_conditions = build_temporal_stage1_conditions(compiled)?;

    // Combine all conditions
    let mut all_conditions = entity_conditions;
    if !temporal_conditions.is_empty() {
        all_conditions = all_conditions.add(temporal_conditions);
    }

    // Build the main SELECT
    let mut select = Query::select();
    select
        .distinct()
        .expr_as(SeaExpr::col((Tea, TemporalEntityAttributes::EntityId)), Alias::new("entity_id"))
        .from_as(TemporalEntityAttributes::Table, Tea)
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            SeaExpr::col((An, AttributeNames::Id)).equals((Tea, TemporalEntityAttributes::AttributeNameId)),
        )
        .join_as(
            JoinType::LeftJoin,
            Entities::Table,
            E,
            SeaExpr::col((E, Entities::Id)).equals((Tea, TemporalEntityAttributes::EntityId)),
        );

    // Conditional JOIN to attribute_history when timerel is present
    let needs_ah_join = compiled.window.interval.is_bounded();
    if needs_ah_join {
        select.join_as(
            JoinType::Join,
            AttributeHistory::Table,
            Ah,
            SeaExpr::col((Ah, AttributeHistory::TeaId)).equals((Tea, TemporalEntityAttributes::Id)),
        );
    }

    select
        // clone: the count statement filters on the same condition
        .cond_where(all_conditions.clone())
        // Stable page order (clause 5.5.9): a `DISTINCT entity_id` projection
        // with no ORDER BY lets LIMIT/OFFSET repeat and skip ids across pages.
        .order_by((Tea, TemporalEntityAttributes::EntityId), Order::Asc)
        .limit(compiled.pagination.limit as u64)
        .offset(compiled.pagination.offset as u64);

    // Build main query SQL
    let (sql, values) = select.build(PostgresQueryBuilder);
    let bind_values = sea_values_to_bind_values(values)?;

    // Build count query if requested
    let count_sql = if compiled.pagination.count == CountPolicy::Emit {
        let mut count_select = Query::select();
        count_select
            .expr(Func::count(SeaExpr::cust("DISTINCT \"tea\".\"entity_id\"")))
            .from_as(TemporalEntityAttributes::Table, Tea)
            .join_as(
                JoinType::Join,
                AttributeNames::Table,
                An,
                SeaExpr::col((An, AttributeNames::Id)).equals((Tea, TemporalEntityAttributes::AttributeNameId)),
            )
            .join_as(
                JoinType::LeftJoin,
                Entities::Table,
                E,
                SeaExpr::col((E, Entities::Id)).equals((Tea, TemporalEntityAttributes::EntityId)),
            );

        if needs_ah_join {
            count_select.join_as(
                JoinType::Join,
                AttributeHistory::Table,
                Ah,
                SeaExpr::col((Ah, AttributeHistory::TeaId)).equals((Tea, TemporalEntityAttributes::Id)),
            );
        }

        count_select.cond_where(all_conditions);
        let (count_sql_str, _) = count_select.build(PostgresQueryBuilder);
        Some(count_sql_str)
    } else {
        None
    };

    Ok(BuiltQuery { sql, count_sql, bind_values })
}

/// Build entity-level conditions for temporal queries (using "e" alias for entities table).
fn build_entity_conditions_for_temporal(compiled: &CompiledTemporalQuery, attribute_names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    let mut condition = Condition::all();

    // ID filter: e.id = ANY($N)
    if !compiled.entities.ids.is_empty() {
        let ids: Vec<&str> = compiled.entities.ids.iter().map(AsRef::as_ref).collect();
        condition = condition.add(SeaExpr::col((E, Entities::Id)).is_in(ids));
    }

    // ID pattern filter: e.id ~ $N
    if let Some(pattern) = &compiled.entities.id_pattern {
        condition = condition.add(regex_match(SeaExpr::col((E, Entities::Id)).into(), SeaExpr::val(pattern.as_str()).into()));
    }

    // Type expression filter
    if let Some(type_expr) = &compiled.entities.type_expr {
        condition = condition.add(type_condition(type_expr, TypesColumn::AliasedEntities));
    }

    // Q expression filter. Correlated on the TEA catalogue's entity_id, which
    // is the driving table for stage 1.
    if let Some(q_expr) = &compiled.entities.q_expr {
        condition = condition.add(q_filter::compile(
            q_expr,
            &QFilterTarget::correlated("\"tea\".\"entity_id\"", "\"e\".\"payload\""),
            attribute_names,
        )?);
    }

    if let Some(geo) = &compiled.entities.geo {
        condition = condition.add(geo_sql::condition(
            geo,
            GeometrySource::EntityPayload { payload_column: "e.payload" },
            NullGeometryPolicy::Exclude,
        )?);
    }

    Ok(condition)
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
    let dataset_column = SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId));
    condition = match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => condition,
        DatasetFilter::DefaultOnly => condition.add(dataset_column.is_null()),
        DatasetFilter::NamedOnly(named) => condition.add(dataset_column.is_in(named.iter().map(Urn::to_string))),
        DatasetFilter::DefaultOrNamed(named) => condition.add(
            Condition::any()
                .add(SeaExpr::col((Tea, TemporalEntityAttributes::DatasetId)).is_in(named.iter().map(Urn::to_string)))
                .add(dataset_column.is_null()),
        ),
    };

    // Temporal filters (timerel + time_at)
    let time_col = history_column(compiled.window.timeproperty);
    let bound = |sql: &'static str, instant: &chrono::DateTime<chrono::Utc>| {
        SeaExpr::cust_with_exprs(sql, [SeaExpr::cust(time_col), SeaExpr::val(instant.to_rfc3339()).into()])
    };
    match &compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(bound("$1 < $2::timestamptz", instant)),
        TimeInterval::After(instant) => condition = condition.add(bound("$1 >= $2::timestamptz", instant)),
        TimeInterval::Between { from, to } => {
            let lower = bound("$1 >= $2::timestamptz", from);
            let upper = bound("$1 < $2::timestamptz", to);
            condition = condition.add(lower).add(upper);
        }
    }

    // Historical scope filtering via EXISTS subquery against scope_history
    if let Some(scope_q) = &compiled.entities.scope_q {
        let target = ScopeTarget::History {
            entity_id_column: "\"tea\".\"entity_id\"",
            interval: compiled.window.interval,
        };
        condition = condition.add(scope_condition(scope_q, target)?);
    }

    Ok(condition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::parser,
        geo::query::GeoQuery,
        scope::query::ScopeQuery,
        temporal::{instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
        type_selection::grammar::TypeExpr,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
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
        compiled.entities.type_expr = Some(TypeExpr::Type(iri("https://uri.etsi.org/ngsi-ld/default-context/Vehicle")));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("SELECT DISTINCT"));
        assert!(result.sql.contains("\"e\".\"types\""));
        assert!(result.sql.contains("JOIN \"attribute_names\""));
    }

    #[test]
    fn entity_id_selection_orders_by_id_for_stable_pagination() {
        // Clause 5.5.9: a `SELECT DISTINCT entity_id` with no ORDER BY lets
        // LIMIT/OFFSET repeat and skip ids across pages.
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("ORDER BY \"tea\".\"entity_id\""), "{}", result.sql);
    }

    #[test]
    fn stage1_timerel_placeholder_is_offset_after_id_filters() {
        let mut compiled = minimal_compiled();
        compiled.entities.ids = ids(&["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"]);
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));

        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();

        assert!(
            !result.sql.contains("< $1::timestamptz"),
            "time placeholder must not collide with earlier id-filter placeholders"
        );
        assert!(
            result.sql.contains("::timestamptz"),
            "temporal comparison should cast bind value to timestamptz"
        );
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

    #[test]
    fn stage1_scope_q_with_or_and_semantics() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/A;/B,/C").expect("valid scope query"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("AND"), "SQL should contain AND for semicolon-separated scopes");
        assert!(result.sql.contains("OR"), "SQL should contain OR for comma-separated scopes");
    }

    #[test]
    fn temporal_scope_q_generates_scope_history_exists() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid").expect("valid scope query"));
        compiled.window.interval = TimeInterval::Before(instant("2024-06-01T00:00:00Z"));
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("scope_history"), "SQL should reference scope_history table");
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
        assert!(result.sql.contains(">="), "Between should have >= on sh.time");
        assert!(result.sql.contains("<"), "Between should have < on sh.time");
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
        // Each scope term should have its own EXISTS subquery
        let exists_count = result.sql.matches("EXISTS").count();
        pretty_assertions::assert_eq!(exists_count, 3, "Three scope terms should produce three EXISTS subqueries");
    }

    #[test]
    fn stage1_q_filter_correlates_on_the_catalogue_entity_id() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/speed\">50").unwrap());
        let names = ResolvedAttributeNames::from_pairs([("https://example.com/speed".parse().expect("valid attribute name"), AttributeNameId::from(7))]);
        let result = build_stage1_entity_ids(&compiled, &names).unwrap();
        assert!(result.sql.contains("\"attribute_current\""), "{}", result.sql);
        assert!(
            result.sql.contains("\"ac\".\"entity_id\" = \"tea\".\"entity_id\""),
            "stage 1 drives from the TEA catalogue, so the probe correlates there: {}",
            result.sql
        );
        assert!(!result.sql.contains("jsonb_path_query_first"), "{}", result.sql);
    }

    #[test]
    fn stage1_q_filter_on_an_unknown_attribute_is_unsatisfiable() {
        let mut compiled = minimal_compiled();
        compiled.entities.q_expr = Some(parser::parse("\"https://example.com/never-written\">50").unwrap());
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(result.sql.contains("FALSE"), "{}", result.sql);
    }

    #[test]
    fn no_scope_q_unchanged() {
        let compiled = minimal_compiled();
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(!result.sql.contains("scope_history"), "No scope_history reference without scopeQ");
    }

    #[test]
    fn a_temporal_geoproperty_iri_containing_an_apostrophe_does_not_terminate_the_sql_literal() {
        let mut compiled = minimal_compiled();
        compiled.entities.geo = Some(GeoQuery {
            georel: GeoRel::Within,
            geometry: point(serde_json::json!([1.0, 2.0])),
            geoproperty_iri: iri("https://example.com/o'brien"),
        });
        let result = build_stage1_entity_ids(&compiled, &ResolvedAttributeNames::default()).unwrap();
        assert!(
            result.sql.contains("o''brien"),
            "apostrophe must be doubled inside the jsonpath literal: {}",
            result.sql
        );
        // Every literal contributes a balanced pair of quotes, and a doubled
        // apostrophe contributes two, so the total is even unless a literal was
        // terminated early.
        assert_eq!(result.sql.matches('\'').count() % 2, 0, "unbalanced SQL literal: {}", result.sql);
    }
}
