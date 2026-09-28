//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to one clause 6.3.10 page of instances per attribute.
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
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp, uri::Uri};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, window::TimeInterval},
};
use andromeda_postgresql_query::scope_predicate::{ScopeTarget, condition as scope_condition};
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    aggregate::{count_all, rank, row_number},
    branch::qualified,
    postgis::st_as_ewkb,
};
use sea_query::{Alias, Asterisk, Condition, DynIden, Expr, ExprTrait, Func, JoinType, NullOrdering, Order, Query, SelectStatement, WindowStatement};

/// Build Stage 2 query: temporal history for a single entity.
///
/// The entity id is bound here rather than by the caller, so the statement owns
/// its whole placeholder sequence and no two functions have to agree on where
/// it starts.
#[must_use]
pub fn build_temporal_history_query(schema: Option<&DynIden>, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let condition = stage2_conditions(schema, entity_id, compiled);
    match compiled.representation.format {
        // The grouped extension format runs the same aggregation pipeline; the
        // builder adds the group-key columns when `representation.grouping` is set.
        TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => build_aggregation_query(schema, compiled, condition),
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => build_history_query(schema, compiled, condition),
    }
}

/// The row-level filter shared by the ranked, unranked and aggregated shapes, so
/// none can reach a row the others cannot.
///
/// Clause 5.2.21: an instance is placed on the requested axis by the column that
/// holds it. `deletedAt` exists only on a row that records a deletion, so asking
/// for that axis asks for those rows and no others; the deletion is recognised
/// by the axis itself rather than by a sentinel value.
fn stage2_conditions(schema: Option<&DynIden>, entity_id: &Uri, compiled: &CompiledTemporalQuery) -> Condition {
    let mut condition = Condition::all().add(Expr::col((Te, TemporalEntities::Id)).eq(entity_id.to_string()));

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

    // Annex C, clause C.5.16 closes with "Vehicle A8311 only entered the Scope
    // within the request interval. Thus in the latter case only Property values
    // are included that have been observed after the Scope has become valid."
    // A Scope query therefore bounds the instances as well as selecting the
    // Entity: an instance is admitted only while the query held at the instant
    // the instance is positioned on, which clause 4.18 answers from the scope
    // event in force there.
    if let Some(scope_q) = &compiled.entities.scope_q {
        let target = ScopeTarget::valid_at(
            (Te, TemporalEntities::TemporalEntityKey),
            compiled.window.timeproperty,
            Expr::col((Ah, axis)),
            schema.cloned(),
        );
        condition = condition.add(scope_condition(scope_q, &target));
    }

    condition
}

/// The catalogue path from the hypertable to the entity grain, shared by the
/// ranked and unranked shapes and the aggregation so none can reach a row the
/// others cannot.
pub(crate) fn base_history_source(select: &mut SelectStatement, schema: Option<&DynIden>) {
    select
        .from(qualified(schema, AttributeHistory::Table).alias(Ah))
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalAttributeStreams::Table),
            S,
            Expr::col((S, TemporalAttributeStreams::StreamKey)).equals((Ah, AttributeHistory::StreamKey)),
        )
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::TemporalEntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
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
        AttributeHistory::GeneratedInstanceId,
        AttributeHistory::VerbatimInstanceId,
        AttributeHistory::ValueNumber,
        AttributeHistory::ValueString,
        AttributeHistory::ValueBoolean,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueDateTime,
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
    select.expr_as(st_as_ewkb(Expr::col((Ah, AttributeHistory::ValueGeometry))), Alias::new("value_geometry_ewkb"));
    select.expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    select.expr(Expr::col((S, TemporalAttributeStreams::DatasetId)));
}

/// Build the history query, narrowed to the page the query compiled.
fn build_history_query(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    match compiled.instances.page {
        InstancePage::Whole => whole_history(schema, compiled, condition),
        InstancePage::LastN { n } => ranked_history(schema, compiled, condition, &last_n_ranking(compiled, n)),
        InstancePage::Forward { size } => ranked_history(schema, compiled, condition, &forward_page_ranking(compiled, size)),
        InstancePage::Backward { size, .. } => ranked_history(schema, compiled, condition, &backward_page_ranking(compiled, size)),
    }
}

/// Every instance in the window, in rendering order.
fn whole_history(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let mut select = Query::select();
    project_base_columns(&mut select);
    base_history_source(&mut select, schema);
    select.cond_where(condition);
    select
        .order_by((An, AttributeNames::Name), Order::Asc)
        .order_by((Ah, axis), Order::Asc)
        .order_by((Ah, AttributeHistory::RecordedAt), Order::Asc)
        .order_by((Ah, AttributeHistory::WriteId), Order::Asc)
        .order_by((Ah, AttributeHistory::WriteOrdinal), Order::Asc);
    select.take()
}

/// How one ranked shape numbers the instances of a stream and which numbers it
/// keeps.
struct Ranking {
    /// The window function every row is numbered by.
    function: Expr,
    /// The per-stream ordering the numbering follows.
    window: WindowStatement,
    /// The instances at the inclusive lower bound of the window, which a forward
    /// page keeps without counting them.
    uncounted: Option<Expr>,
    /// The largest number kept.
    kept: u32,
}

/// Clauses 5.7.3.4 and 5.7.4.4: a `lastN` the broker answers in full is
/// exactly the last `n` instances.
///
/// The ranking breaks ties on the hypertable's unique key,
/// `(recorded_at, write_id, write_ordinal)`, so two instances sharing a value on
/// the requested axis are kept or dropped the same way on every execution.
fn last_n_ranking(compiled: &CompiledTemporalQuery, n: u32) -> Ranking {
    let axis = axis_column(compiled.window.timeproperty);
    let mut window = WindowStatement::partition_by((Ah, AttributeHistory::StreamKey));
    window
        .order_by((Ah, axis), Order::Desc)
        .order_by((Ah, AttributeHistory::RecordedAt), Order::Desc)
        .order_by((Ah, AttributeHistory::WriteId), Order::Desc)
        .order_by((Ah, AttributeHistory::WriteOrdinal), Order::Desc);
    Ranking {
        function: row_number(),
        window,
        uncounted: None,
        kept: n,
    }
}

/// Clause 6.3.10 forwards: the least recent `size` instances of every stream.
///
/// The instances are ranked on the axis alone, so every instance of the group
/// sharing the timestamp at the page edge carries the rank of the group's first
/// member and the page keeps the group whole: a client resuming with
/// `timerel=after&timeAt=<range-end>` (an inclusive bound, clause 4.11) then
/// meets the group again rather than finding part of it missing. For the same
/// reason the instances sitting exactly on the inclusive lower bound of the
/// window are not counted: they are the group the previous page ended on, and
/// counting them could stall a resumed walk on that one timestamp.
fn forward_page_ranking(compiled: &CompiledTemporalQuery, size: u32) -> Ranking {
    let axis = axis_column(compiled.window.timeproperty);
    let mut window = WindowStatement::partition_by((Ah, AttributeHistory::StreamKey));
    window.order_by_with_nulls((Ah, axis), Order::Asc, NullOrdering::Last);
    let uncounted = match compiled.window.interval {
        TimeInterval::After(lower) | TimeInterval::Between { from: lower, .. } => {
            Some(Func::sum(Expr::case(Expr::col((Ah, axis)).eq(lower), 1).finally(0)).into())
        }
        TimeInterval::Unbounded | TimeInterval::Before(_) => None,
    };
    Ranking {
        function: rank(),
        window,
        uncounted,
        kept: size,
    }
}

/// Clause 6.3.10 backwards: the most recent `size` instances of every stream,
/// ranked on the axis alone so the group sharing the timestamp at the page's
/// lower edge is kept whole. `timerel=before` is an exclusive bound (clause
/// 4.11), so a client resuming with `timeAt=<range-end>` then neither meets the
/// group again nor misses any of it.
fn backward_page_ranking(compiled: &CompiledTemporalQuery, size: u32) -> Ranking {
    let axis = axis_column(compiled.window.timeproperty);
    let mut window = WindowStatement::partition_by((Ah, AttributeHistory::StreamKey));
    window.order_by_with_nulls((Ah, axis), Order::Desc, NullOrdering::Last);
    Ranking {
        function: rank(),
        window,
        uncounted: None,
        kept: size,
    }
}

/// The history narrowed by `ranking`, with every stream's total in the window
/// beside it so the page can tell whether it left instances out.
fn ranked_history(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, condition: Condition, ranking: &Ranking) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let partition = || WindowStatement::partition_by((Ah, AttributeHistory::StreamKey));

    let mut ranked = Query::select();
    project_base_columns(&mut ranked);
    // clone: sea-query owns every expression and window it renders.
    ranked.expr_window_as(ranking.function.clone(), ranking.window.clone(), Alias::new("rn"));
    ranked.expr_window_as(count_all(), partition(), Alias::new("partition_total"));
    if let Some(uncounted) = &ranking.uncounted {
        // clone: same.
        ranked.expr_window_as(uncounted.clone(), partition(), Alias::new("uncounted"));
    }
    base_history_source(&mut ranked, schema);
    ranked.cond_where(condition);

    let ranked_alias = Alias::new("ranked");
    let position = Expr::col((ranked_alias.clone(), Alias::new("rn")));
    let counted = if ranking.uncounted.is_some() {
        position.sub(Expr::col((ranked_alias.clone(), Alias::new("uncounted"))))
    } else {
        position
    };
    let mut outer = Query::select();
    outer
        .expr(Expr::col((ranked_alias.clone(), Asterisk)))
        .from_subquery(ranked.take(), ranked_alias.clone())
        .and_where(counted.lte(i64::from(ranking.kept)))
        .order_by((ranked_alias.clone(), Alias::new("attribute_name")), Order::Asc)
        .order_by((ranked_alias.clone(), axis), Order::Asc)
        .order_by((ranked_alias.clone(), AttributeHistory::RecordedAt), Order::Asc)
        .order_by((ranked_alias.clone(), AttributeHistory::WriteId), Order::Asc)
        .order_by((ranked_alias, AttributeHistory::WriteOrdinal), Order::Asc);
    outer.take()
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
        build_temporal_history_query(None, &vehicle_id(), compiled).to_string(PostgresQueryBuilder)
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
            sql.contains("JOIN \"temporal_entities\" AS \"te\" ON \"te\".\"temporal_entity_key\" = \"s\".\"temporal_entity_key\""),
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
        assert!(sql.contains("\"ah\".\"generated_instance_id\""), "{sql}");
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
        compiled.instances.page = InstancePage::LastN { n: 5 };
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
        compiled.instances.page = InstancePage::LastN { n: 5 };
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ranked\""), "{sql}");
        assert!(sql.contains("ROW_NUMBER()"), "{sql}");
        assert!(sql.contains("\"ranked\".\"rn\" <="), "{sql}");
    }

    /// Clause 6.3.10 forwards: without `lastN` the page is the least recent
    /// instances, ranked ascending on the axis alone so a group sharing the
    /// page-edge timestamp shares one rank and is kept whole.
    #[test]
    fn a_forward_page_ranks_ascending_on_the_axis_alone() {
        let mut compiled = minimal_compiled();
        compiled.instances.page = InstancePage::Forward { size: 100 };
        let sql = sql_of(&compiled);
        assert!(
            sql.contains("RANK() OVER ( PARTITION BY \"ah\".\"stream_key\" ORDER BY \"ah\".\"observed_at\" ASC NULLS LAST )"),
            "{sql}"
        );
        assert!(!sql.contains("ROW_NUMBER()"), "{sql}");
        assert!(sql.contains("\"ranked\".\"rn\" <= 100"), "{sql}");
        assert!(!sql.contains("uncounted"), "an unbounded window has no lower bound to leave uncounted: {sql}");
    }

    /// The instances on the inclusive lower bound of `after`/`between` are the
    /// group a previous forward page ended on, so they are kept without being
    /// counted and a resumed walk always advances.
    #[test]
    fn a_forward_page_leaves_the_inclusive_lower_bound_uncounted() {
        for interval in [
            TimeInterval::After("2024-01-15T10:00:00Z".parse().expect("valid instant")),
            TimeInterval::Between {
                from: "2024-01-15T10:00:00Z".parse().expect("valid instant"),
                to: "2024-01-16T10:00:00Z".parse().expect("valid instant"),
            },
        ] {
            let mut compiled = minimal_compiled();
            compiled.instances.page = InstancePage::Forward { size: 100 };
            compiled.window.interval = interval;
            let sql = sql_of(&compiled);
            assert!(
                sql.contains("SUM((CASE WHEN (\"ah\".\"observed_at\" = '2024-01-15 10:00:00.000000 +00:00') THEN 1 ELSE 0 END)) OVER ( PARTITION BY \"ah\".\"stream_key\" ) AS \"uncounted\""),
                "{sql}"
            );
            assert!(sql.contains("\"ranked\".\"rn\" - \"ranked\".\"uncounted\" <= 100"), "{sql}");
        }
    }

    /// `before` is an exclusive bound, so a backward page counts every instance
    /// and ranks descending on the axis alone.
    #[test]
    fn a_backward_page_ranks_descending_on_the_axis_alone() {
        let mut compiled = minimal_compiled();
        compiled.instances.page = InstancePage::Backward { size: 1000, requested: 5000 };
        compiled.window.interval = TimeInterval::Before("2024-01-15T10:00:00Z".parse().expect("valid instant"));
        let sql = sql_of(&compiled);
        assert!(
            sql.contains("RANK() OVER ( PARTITION BY \"ah\".\"stream_key\" ORDER BY \"ah\".\"observed_at\" DESC NULLS LAST )"),
            "{sql}"
        );
        assert!(sql.contains("\"ranked\".\"rn\" <= 1000"), "{sql}");
        assert!(!sql.contains("uncounted"), "{sql}");
    }

    /// Every ranked shape reports each stream's total in the window, which is
    /// how the page tells whether it left an instance out.
    #[test]
    fn every_ranked_shape_reports_the_stream_total() {
        for page in [
            InstancePage::LastN { n: 5 },
            InstancePage::Forward { size: 5 },
            InstancePage::Backward { size: 5, requested: 50 },
        ] {
            let mut compiled = minimal_compiled();
            compiled.instances.page = page;
            let sql = sql_of(&compiled);
            assert!(
                sql.contains("COUNT(*) OVER ( PARTITION BY \"ah\".\"stream_key\" ) AS \"partition_total\""),
                "{page:?}: {sql}"
            );
        }
    }

    /// Every instance in the window is read when no page applies.
    #[test]
    fn the_whole_history_is_not_ranked() {
        let sql = sql_of(&minimal_compiled());
        assert!(!sql.contains("RANK()"), "{sql}");
        assert!(!sql.contains("ROW_NUMBER()"), "{sql}");
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
