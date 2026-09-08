//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to the last `N` instances per attribute.
//!
//! The statement reads the rebuilt hypertable through the temporal catalogue
//! alone - `attribute_history` to `temporal_attribute_streams` to
//! `temporal_entities` - so an entity whose current state was deleted (clause
//! 5.6.6) still answers with its evolution. Every row carries its own
//! `attribute_kind` and `value_kind`, which is what the reconstruction reads:
//! a historical instance's kind is a fact of the row, never re-derived from a
//! catalogue that a later type change would have overwritten.

use crate::{
    aggregation::build_aggregation_query,
    alias::{Ah, An, S, Te},
    axis::axis_column,
};
use andromeda_context_space::context_space_key::ContextSpaceKey;
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
};
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    aggregate::{count_all, row_number},
    postgis::st_as_ewkb,
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, JoinType, Order, Query, SelectStatement, WindowStatement};

/// Build Stage 2 query: temporal history for a single entity.
///
/// The entity id is bound here rather than by the caller, so the statement owns
/// its whole placeholder sequence and no two functions have to agree on where
/// it starts.
#[must_use]
pub fn build_temporal_history_query(context_space_key: ContextSpaceKey, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let condition = stage2_conditions(context_space_key, entity_id, compiled);
    match compiled.representation.format {
        TemporalFormat::AggregatedValues => build_aggregation_query(compiled, condition),
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => build_history_query(compiled, condition),
    }
}

/// The row-level filter shared by the ranked, unranked and aggregated shapes, so
/// none can reach a row the others cannot.
///
/// Clause 5.2.21: an instance is placed on the requested axis by the column that
/// holds it. `deletedAt` exists only on a row that records a deletion, so asking
/// for that axis asks for those rows and no others; the deletion is recognised
/// by the axis itself rather than by a sentinel value.
fn stage2_conditions(context_space_key: ContextSpaceKey, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Condition {
    let mut condition = Condition::all()
        .add(Expr::col((Te, TemporalEntities::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .add(Expr::col((S, TemporalAttributeStreams::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .add(Expr::col((Ah, AttributeHistory::ContextSpaceKey)).eq(context_space_key.as_i64()))
        .add(Expr::col((Te, TemporalEntities::Id)).eq(entity_id.to_string()));

    if compiled.window.timeproperty == TimeProp::Deleted {
        condition = condition.add(Expr::col((Ah, AttributeHistory::DeletedAt)).is_not_null());
    }

    let axis = axis_column(compiled.window.timeproperty);
    match compiled.window.interval {
        TimeInterval::Unbounded => {}
        TimeInterval::Before(instant) => condition = condition.add(Expr::col((Ah, axis)).lt(instant)),
        TimeInterval::After(instant) => condition = condition.add(Expr::col((Ah, axis)).gte(instant)),
        TimeInterval::Between { from, to } => {
            condition = condition.add(Expr::col((Ah, axis)).gte(from)).add(Expr::col((Ah, axis)).lt(to));
        }
    }

    if !compiled.instances.attr_filter.is_empty() {
        let names = compiled.instances.attr_filter.iter().map(iri_rs::Iri::as_str);
        condition = condition.add(Expr::col((An, AttributeNames::Name)).is_in(names));
    }

    // datasetId filter (clause 4.5.5): the default instance is stored as a
    // NULL `dataset_id`, so it is tested separately from the named ones.
    condition = match DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter) {
        DatasetFilter::Unfiltered => condition,
        DatasetFilter::DefaultOnly => condition.add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_null()),
        DatasetFilter::NamedOnly(named) => condition.add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string))),
        DatasetFilter::DefaultOrNamed(named) => condition.add(
            Condition::any()
                .add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_in(named.iter().map(ToString::to_string)))
                .add(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_null()),
        ),
    };

    condition
}

/// The catalogue path from the hypertable to the entity grain, shared by the
/// ranked and unranked shapes and the aggregation so none can reach a row the
/// others cannot.
pub(crate) fn base_history_source(select: &mut SelectStatement) {
    select
        .from_as(AttributeHistory::Table, Ah)
        .join_as(
            JoinType::Join,
            TemporalAttributeStreams::Table,
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((Ah, AttributeHistory::StreamKey)),
        )
        .join_as(
            JoinType::Join,
            TemporalEntities::Table,
            Te,
            Expr::col((Te, TemporalEntities::EntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        );
}

/// Project every column a history row carries, before the optional `lastN`
/// ranking.
///
/// The two instance-identity columns are projected side by side: a broker-minted
/// identifier is a `UUID` rendered through the permanent URN template, a
/// client-supplied one (clause 4.5.7 requires it to survive) is the stored text,
/// and exactly one of them is populated on every row.
fn project_base_columns(select: &mut SelectStatement) {
    for column in [
        AttributeHistory::RecordedAt,
        AttributeHistory::StreamKey,
        AttributeHistory::AttributeKind,
        AttributeHistory::ValueKind,
        AttributeHistory::ObservedAt,
        AttributeHistory::CreatedAt,
        AttributeHistory::ModifiedAt,
        AttributeHistory::DeletedAt,
        AttributeHistory::GeneratedInstanceUuid,
        AttributeHistory::VerbatimInstanceId,
        AttributeHistory::ValueNumber,
        AttributeHistory::ValueString,
        AttributeHistory::ValueBool,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueDatetime,
        AttributeHistory::ValueUri,
        AttributeHistory::UnitCodeId,
        AttributeHistory::Metadata,
        AttributeHistory::WriteId,
        AttributeHistory::WriteOrdinal,
    ] {
        select.expr(Expr::col((Ah, column)));
    }
    // The geometry is serialised back to EWKB in the statement; the store
    // decodes that wire form into a GeoJSON value in Rust.
    select.expr_as(st_as_ewkb(Expr::col((Ah, AttributeHistory::ValueGeo))), Alias::new("value_geo_ewkb"));
    select.expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    select.expr(Expr::col((S, TemporalAttributeStreams::DatasetId)));
}

/// Build the history query, with the optional `lastN` ranking.
fn build_history_query(compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);

    if let Some(last_n) = compiled.instances.last_n {
        // A total order over the instances of one stream, so the ranking that
        // picks the last N and the rendering order do not depend on the plan.
        // `(recorded_at, write_id, write_ordinal)` is the hypertable's unique
        // key, so it separates every pair of rows, including two instances that
        // share a value on the requested axis.
        let mut rank_window = WindowStatement::partition_by((Ah, AttributeHistory::StreamKey));
        rank_window
            .order_by((Ah, axis), Order::Desc)
            .order_by((Ah, AttributeHistory::RecordedAt), Order::Desc)
            .order_by((Ah, AttributeHistory::WriteId), Order::Desc)
            .order_by((Ah, AttributeHistory::WriteOrdinal), Order::Desc);

        let mut ranked = Query::select();
        project_base_columns(&mut ranked);
        ranked.expr_window_as(row_number(), rank_window, Alias::new("rn"));
        ranked.expr_window_as(
            count_all(),
            WindowStatement::partition_by((Ah, AttributeHistory::StreamKey)),
            Alias::new("partition_total"),
        );
        base_history_source(&mut ranked);
        ranked.cond_where(condition);

        let ranked_alias = Alias::new("ranked");
        let mut outer = Query::select();
        outer
            .expr(Expr::col((ranked_alias.clone(), Asterisk)))
            .from_subquery(ranked.take(), ranked_alias.clone())
            .and_where(Expr::col((ranked_alias.clone(), Alias::new("rn"))).lte(i64::from(last_n)))
            .order_by((ranked_alias.clone(), Alias::new("attribute_name")), Order::Asc)
            .order_by((ranked_alias.clone(), axis), Order::Asc)
            .order_by((ranked_alias.clone(), AttributeHistory::RecordedAt), Order::Asc)
            .order_by((ranked_alias.clone(), AttributeHistory::WriteId), Order::Asc)
            .order_by((ranked_alias, AttributeHistory::WriteOrdinal), Order::Asc);
        outer.take()
    } else {
        let mut select = Query::select();
        project_base_columns(&mut select);
        base_history_source(&mut select);
        select.cond_where(condition);
        select
            .order_by((An, AttributeNames::Name), Order::Asc)
            .order_by((Ah, axis), Order::Asc)
            .order_by((Ah, AttributeHistory::RecordedAt), Order::Asc)
            .order_by((Ah, AttributeHistory::WriteId), Order::Asc)
            .order_by((Ah, AttributeHistory::WriteOrdinal), Order::Asc);
        select.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        temporal::{aggregation_period::AggregationPeriod, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use iri_rs::IriBuf;
    use sea_query::PostgresQueryBuilder;

    /// The one entity these tests build a history statement for.
    fn vehicle_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id")
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

    /// The stage-2 statement rendered as SQL with its values inlined, which is
    /// what the shape assertions read.
    fn sql_of(compiled: &CompiledTemporalQuery) -> String {
        let context_space_key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        build_temporal_history_query(context_space_key, &vehicle_id(), compiled).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn build_temporal_history_no_filters() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"te\".\"id\" ="), "{sql}");
        assert!(sql.contains("attribute_history"), "{sql}");
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
        assert!(sql.contains("ORDER BY"), "{sql}");
    }

    /// Clause 5.6.6 leaves the evolution standing when the current entity is
    /// deleted, so stage 2 reaches the entity grain through the temporal
    /// catalogue and names neither `entities` nor the retired one.
    #[test]
    fn history_reaches_the_entity_through_the_temporal_catalogue() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains("JOIN \"temporal_attribute_streams\" AS \"s\" ON \"s\".\"stream_key\" = \"ah\".\"stream_key\""),
            "{sql}"
        );
        assert!(
            sql.contains("JOIN \"temporal_entities\" AS \"te\" ON \"te\".\"entity_key\" = \"s\".\"temporal_entity_key\""),
            "{sql}"
        );
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
        assert!(!sql.contains("\"entities\" AS"), "{sql}");
    }

    /// The kind of a historical instance is a fact of the row, because the
    /// catalogue records only the current kind of the stream and would
    /// reinterpret older rows through it.
    #[test]
    fn every_row_carries_its_own_attribute_and_value_kind() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"ah\".\"attribute_kind\""), "{sql}");
        assert!(sql.contains("\"ah\".\"value_kind\""), "{sql}");
        assert!(!sql.contains("attribute_type"), "{sql}");
        assert!(!sql.contains("value_type"), "{sql}");
    }

    /// Clause 4.5.7: a client-supplied `instanceId` survives verbatim while a
    /// broker-minted one is a UUID rendered through the permanent template, so
    /// both columns are projected and the reconstruction picks the populated one.
    #[test]
    fn both_halves_of_the_instance_identity_are_projected() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"ah\".\"generated_instance_uuid\""), "{sql}");
        assert!(sql.contains("\"ah\".\"verbatim_instance_id\""), "{sql}");
        assert!(!sql.contains("\"instance_id\""), "{sql}");
    }

    /// All four axes of clause 5.2.21 are rendered, and the operational
    /// `recorded_at` is projected for ordering only - never as a substitute for
    /// an axis the row does not carry.
    #[test]
    fn all_four_time_axes_are_projected_without_substitution() {
        let sql = sql_of(&minimal_compiled());
        for column in ["observed_at", "created_at", "modified_at", "deleted_at", "recorded_at"] {
            assert!(sql.contains(&format!("\"ah\".\"{column}\"")), "{column} missing: {sql}");
        }
        assert!(!sql.contains("COALESCE"), "{sql}");
    }

    /// Clause 5.2.21: the deletion axis exists only on the rows that record a
    /// deletion, and those are recognised by the axis rather than by a null URN
    /// stored in a value column.
    #[test]
    fn the_deleted_at_axis_selects_the_rows_that_carry_it() {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = TimeProp::Deleted;
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"deleted_at\" IS NOT NULL"), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:null"), "{sql}");
    }

    #[test]
    fn last_n_ranks_on_the_write_identity_tiebreaker() {
        // Two instances sharing a timestamp must be kept or dropped consistently
        // across executions, so the ROW_NUMBER window that picks the last N
        // breaks ties on the hypertable's unique key rather than leaving it to
        // the plan.
        let mut compiled = minimal_compiled();
        compiled.instances.last_n = Some(5);
        let sql = sql_of(&compiled);
        assert!(sql.contains("ROW_NUMBER()"), "{sql}");
        assert!(sql.contains("PARTITION BY \"ah\".\"stream_key\""), "{sql}");
        assert!(sql.contains("\"ah\".\"write_ordinal\" DESC"), "{sql}");
    }

    #[test]
    fn build_temporal_history_before() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Before("2024-01-15T10:00:00Z".parse().expect("valid instant"));
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"observed_at\" <"), "{sql}");
    }

    #[test]
    fn build_temporal_history_after() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After("2024-01-15T10:00:00Z".parse().expect("valid instant"));
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"observed_at\" >="), "{sql}");
    }

    #[test]
    fn build_temporal_history_between() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Between {
            from: "2024-01-15T10:00:00Z".parse().expect("valid instant"),
            to: "2024-01-15T12:00:00Z".parse().expect("valid instant"),
        };
        let sql = sql_of(&compiled);
        assert!(sql.contains(">="), "{sql}");
        assert!(sql.contains('<'), "{sql}");
    }

    /// Clause 4.11 defaults the selector to `observedAt`; each axis is filtered
    /// on its own column, so an instance with no `observedAt` matches no
    /// `observedAt` window.
    #[test]
    fn each_time_property_filters_its_own_column() {
        let expected = [
            (TimeProp::Observed, "\"ah\".\"observed_at\" <"),
            (TimeProp::Created, "\"ah\".\"created_at\" <"),
            (TimeProp::Modified, "\"ah\".\"modified_at\" <"),
            (TimeProp::Deleted, "\"ah\".\"deleted_at\" <"),
        ];
        for (prop, predicate) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before("2024-01-15T10:00:00Z".parse().expect("valid instant"));
            let sql = sql_of(&compiled);
            assert!(sql.contains(predicate), "{prop}: {sql}");
        }
    }

    #[test]
    fn build_temporal_history_with_attrs() {
        let mut compiled = minimal_compiled();
        compiled.instances.attr_filter = vec![IriBuf::new("https://uri.etsi.org/ngsi-ld/default-context/speed".to_string()).expect("valid IRI")];
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"an\".\"name\" IN ("), "{sql}");
    }

    #[test]
    fn build_temporal_history_with_last_n() {
        let mut compiled = minimal_compiled();
        compiled.instances.last_n = Some(5);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ranked\""), "{sql}");
        assert!(sql.contains("ROW_NUMBER()"), "{sql}");
        assert!(sql.contains("\"ranked\".\"rn\" <="), "{sql}");
    }

    /// Clause 4.5.5: `@none` selects the instance that carries no `datasetId`,
    /// which the stream catalogue holds as a NULL column, so it cannot be tested
    /// by the same membership predicate as a named dataset.
    #[test]
    fn dataset_id_none_selects_the_default_instance() {
        let selectors = |raw: &[&str]| -> Vec<DatasetSelector> { raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid")).collect() };

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"s\".\"dataset_id\" IS NULL"), "{sql}");
        assert!(!sql.contains("\"s\".\"dataset_id\" IN"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"s\".\"dataset_id\" IN"), "{sql}");
        assert!(!sql.contains("\"s\".\"dataset_id\" IS NULL"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none", "urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"s\".\"dataset_id\" IN"), "{sql}");
        assert!(sql.contains("OR \"s\".\"dataset_id\" IS NULL"), "{sql}");
    }

    #[test]
    fn build_aggregation_query_with_period() {
        let mut compiled = minimal_compiled();
        compiled.representation.format = TemporalFormat::AggregatedValues;
        compiled.representation.aggr_methods = vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max];
        compiled.representation.aggr_period_duration = AggregationPeriod::parse("PT1H").expect("valid period");
        let sql = sql_of(&compiled);
        assert!(sql.contains("time_bucket"), "{sql}");
        assert!(sql.contains("bucket_start"), "{sql}");
        assert!(sql.contains("agg_avg"), "{sql}");
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
    }

    #[test]
    fn build_aggregation_query_whole_range() {
        let mut compiled = minimal_compiled();
        compiled.representation.format = TemporalFormat::AggregatedValues;
        compiled.representation.aggr_methods = vec![AggrMethod::TotalCount];
        let sql = sql_of(&compiled);
        assert!(sql.contains("agg_total_count"), "{sql}");
        assert!(sql.contains("GROUP BY"), "{sql}");
    }
}
