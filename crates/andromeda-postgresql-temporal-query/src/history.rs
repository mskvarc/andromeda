//! Stage 2 of a temporal query: attribute history for one entity, optionally
//! narrowed to one clause 6.3.10 page of instances per attribute.
//!
//! The statement reads the attribute source of the branch, which reaches the
//! hypertable through the temporal catalogue alone, so an entity whose
//! current state was deleted (clause 5.6.6) still answers with its evolution,
//! and on the live branch carries the entries still queued for it. Every row
//! carries its own `attribute_kind` and `value_kind`, which is what the
//! reconstruction reads: a historical instance's kind is a fact of the row,
//! never re-derived from a catalogue that a later type change would have
//! overwritten.

use crate::{
    aggregation::build_aggregation_query,
    alias::{Ah, An},
    axis::axis_column,
    stream_page::{Walk, last_n_history, project_whole_streams, walked_history},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, period_budget::PeriodBudget, window::TimeInterval},
};
use andromeda_postgresql_query::{
    dataset_predicate::dataset_condition,
    history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, grain::HistoryGrain},
    scope_intervals::held_at,
};
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::postgis::st_as_ewkb;
use sea_query::{Alias, Condition, Expr, ExprTrait, JoinType, Order, Query, SelectStatement};

/// Build Stage 2 query: temporal history for a single entity.
///
/// The entity id is bound here rather than by the caller, so the statement owns
/// its whole placeholder sequence and no two functions have to agree on where
/// it starts.
#[must_use]
pub fn build_temporal_history_query(branch: &HistoryBranch, entity_id: &Identifier, compiled: &CompiledTemporalQuery) -> SelectStatement {
    // The ranked, unranked and aggregated shapes read the same entity's rows
    // among the same represented instances, so none can reach a row the others
    // cannot.
    let of = HistoryOf::Entity(Expr::val(entity_id.as_str()));
    let condition = represented_instances(branch, &of, compiled);
    match compiled.representation.format {
        // The grouped extension format runs the same aggregation pipeline; the
        // builder adds the group-key columns when `representation.grouping` is set.
        TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => build_aggregation_query(branch, &of, compiled, &condition),
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => build_history_query(branch, &of, compiled, condition),
    }
}

/// The Attribute instances the temporal representation of the entity `of`
/// names carries, over the rows of [`base_history_source`]: stage 2 reads
/// them, and stage 1 asks whether a projection leaves an entity any of them.
///
/// Clause 5.2.21: an instance is placed on the requested axis by the column that
/// holds it, and an instance that holds no instant there is represented by no
/// read on that axis. Clause 4.5.7 places every instance of the representation
/// "at a particular point in time, which is recorded as a Temporal Property of
/// the instance"; clause 6.3.10 pages it by that instant and clause 4.5.9 pairs
/// each value with it. Such an instance stays readable on `createdAt` and
/// `modifiedAt`, which the broker stamps on every instance (clause 4.8).
/// `deletedAt` exists only on a row that records a deletion, so asking for that
/// axis asks for those rows and no others; the deletion is recognised by the
/// axis itself rather than by a sentinel value.
pub(crate) fn represented_instances(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery) -> Condition {
    let axis = axis_column(compiled.window.timeproperty);
    let mut condition = Condition::all().add(Expr::col((Ah, axis)).is_not_null());

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

    // Clause 5.7.4.4 step S8 (5.7.3.4 for a retrieval): the `datasetId`
    // parameter keeps only the instances of the datasets it names. It filters
    // the instances an Entity is represented with, never which Entities are.
    condition = condition.add(dataset_condition(
        &Expr::col((Ah, HistoryGrain::DatasetId)),
        &DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter),
    ));

    // Annex C, clause C.5.16 closes with "Vehicle A8311 only entered the Scope
    // within the request interval. Thus in the latter case only Property values
    // are included that have been observed after the Scope has become valid."
    // A Scope query therefore bounds the instances as well as selecting the
    // Entity: an instance is admitted only while the query held at the instant
    // the instance is positioned on, which clause 4.18 answers from the scope
    // event in force there. The query is judged over the Entity's scope
    // intervals once, so an instance costs a lookup among them.
    if let Some(scope_q) = &compiled.entities.scope_q {
        condition = condition.add(held_at(scope_q, of, compiled.window.timeproperty, Expr::col((Ah, axis)), branch));
    }

    condition
}

/// The attribute rows `of` reads beside the name of their attribute, shared by
/// the ranked and unranked shapes and the aggregation so none can reach a row
/// the others cannot.
pub(crate) fn base_history_source(select: &mut SelectStatement, branch: &HistoryBranch, of: &HistoryOf) {
    select.from(attribute_rows(branch, of, Ah)).join_as(
        JoinType::Join,
        AttributeNames::Table,
        An,
        Expr::col((An, AttributeNames::AttributeNameKey)).equals((Ah, HistoryGrain::AttributeNameKey)),
    );
}

/// Project every column a history row carries.
///
/// The two instance-identity columns are projected side by side: a broker-minted
/// identifier is a `UUID` rendered through the permanent URN template, a
/// client-supplied one (clause 4.5.7 requires it to survive) is the stored text,
/// and exactly one of them is populated on every row.
pub(crate) fn project_base_columns(select: &mut SelectStatement) {
    for column in [
        AttributeHistory::RecordedAt,
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
        AttributeHistory::ValueGeometryMembers,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueDateTime,
        AttributeHistory::ValueUri,
        AttributeHistory::UnitCodeId,
        AttributeHistory::ValueTypeKey,
        AttributeHistory::ObjectTypeKeys,
        AttributeHistory::ProofSeals,
        AttributeHistory::ExpiresAt,
        AttributeHistory::SubAttributes,
        AttributeHistory::WriteId,
        AttributeHistory::WriteOrdinal,
    ] {
        select.expr(Expr::col((Ah, column)));
    }
    // The geometry is serialised back to EWKB in the statement; the store
    // decodes that wire form into a GeoJSON value in Rust.
    select.expr_as(st_as_ewkb(Expr::col((Ah, AttributeHistory::ValueGeometry))), Alias::new("value_geometry_ewkb"));
    select.expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    select.expr(Expr::col((Ah, HistoryGrain::DatasetId)));
}

/// Build the history query, narrowed to the page the query compiled; a page
/// of periods, which an instance representation never compiles to, counts its
/// periods as instances.
fn build_history_query(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    match compiled.instances.page {
        InstancePage::Whole => whole_history(branch, of, compiled, condition),
        InstancePage::LastN { n } => last_n_history(branch, of, compiled, &condition, n),
        InstancePage::Forward { size }
        | InstancePage::Periods {
            budget: PeriodBudget { periods: size, .. },
        } => walked_history(branch, of, compiled, &condition, Walk::Forward, size),
        InstancePage::Backward { size, .. }
        | InstancePage::PeriodsBackward {
            budget: PeriodBudget { periods: size, .. },
            ..
        } => walked_history(branch, of, compiled, &condition, Walk::Backward, size),
    }
}

/// Every instance in the window, in rendering order.
fn whole_history(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let mut select = Query::select();
    project_base_columns(&mut select);
    project_whole_streams(&mut select);
    base_history_source(&mut select, branch, of);
    select.cond_where(condition);
    select
        .order_by((An, AttributeNames::Name), Order::Asc)
        .order_by((Ah, axis), Order::Asc)
        .order_by((Ah, AttributeHistory::RecordedAt), Order::Asc)
        .order_by((Ah, AttributeHistory::WriteId), Order::Asc)
        .order_by((Ah, AttributeHistory::WriteOrdinal), Order::Asc);
    select.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        scope::query::ScopeQuery,
        temporal::{aggregation_period::AggregationPeriod, instance_selection::InstanceSelection, representation::Representation, window::TemporalWindow},
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use iri_rs::IriBuf;
    use sea_query::{IntoIden, PostgresQueryBuilder};
    use std::num::NonZeroU32;

    /// The one entity these tests build a history statement for.
    fn vehicle_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id")
    }

    /// `n` as a client `lastN`.
    fn count(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).expect("a positive count")
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
        build_temporal_history_query(&HistoryBranch::live(), &vehicle_id(), compiled).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn build_temporal_history_no_filters() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"re\".\"id\" ="), "{sql}");
        assert!(sql.contains("attribute_history"), "{sql}");
        assert!(sql.contains("JOIN \"attribute_names\""), "{sql}");
        assert!(sql.contains("ORDER BY"), "{sql}");
    }

    /// Clause 5.6.6 leaves the evolution standing when the current entity is
    /// deleted, so stage 2 reaches the entity grain through the temporal
    /// catalogue and not through the current-state tables.
    #[test]
    fn history_reaches_the_entity_through_the_temporal_catalogue() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains("JOIN \"temporal_attribute_streams\" AS \"rs\" ON \"rs\".\"stream_key\" = \"rh\".\"stream_key\""),
            "{sql}"
        );
        assert!(
            sql.contains("JOIN \"temporal_entities\" AS \"re\" ON \"re\".\"temporal_entity_key\" = \"rs\".\"temporal_entity_key\""),
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
        assert!(!sql.contains("\"attribute_type\""), "{sql}");
        assert!(!sql.contains("\"value_type\""), "{sql}");
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

    /// Clause 4.5.5: `@none` selects the instance that carries no `datasetId`,
    /// which the stream catalogue holds as a NULL column, so it cannot be tested
    /// by the same membership predicate as a named dataset.
    #[test]
    fn dataset_id_none_selects_the_default_instance() {
        let selectors = |raw: &[&str]| -> Vec<DatasetSelector> { raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid")).collect() };

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"dataset_id\" IS NULL"), "{sql}");
        assert!(!sql.contains("\"ah\".\"dataset_id\" IN"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"dataset_id\" IN"), "{sql}");
        assert!(!sql.contains("\"ah\".\"dataset_id\" IS NULL"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none", "urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"ah\".\"dataset_id\" IN"), "{sql}");
        assert!(sql.contains("OR \"ah\".\"dataset_id\" IS NULL"), "{sql}");
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
        assert!(sql.contains("HAVING (COUNT(*)) > (0)"), "{sql}");
    }

    /// Annex C clause C.5.16 bounds the instances by the Scope in force at each
    /// of them, which the statement reads as the Entity's scope intervals,
    /// judged once for the Entity it names, rather than as a probe per
    /// instance. The recorded events are read once; the other reads of
    /// `scope_history` judge a queued Scope assertion, two axes each read for
    /// the head and for its state.
    #[test]
    fn a_temporal_scope_q_reads_the_scope_intervals_once_per_entity() {
        let mut compiled = minimal_compiled();
        compiled.entities.scope_q = Some(ScopeQuery::parse("/Madrid/#").expect("valid scope query"));
        let sql = sql_of(&compiled);
        assert_eq!(sql.matches(r#"FROM "scope_history" AS "rsh""#).count(), 1, "{sql}");
        assert_eq!(sql.matches(r#"FROM "scope_history" AS "pj""#).count(), 4, "{sql}");
        assert!(!sql.contains("sh_next"), "{sql}");
        assert!(
            sql.contains(
                r#""scope_intervals"."starts" <= "ah"."observed_at" AND ("scope_intervals"."ends" IS NULL OR "ah"."observed_at" < "scope_intervals"."ends")"#
            ),
            "{sql}"
        );
        assert!(
            !sql.contains(r#""re"."id" = "ah"."entity_id""#),
            "the intervals do not follow the instance: {sql}"
        );
    }

    /// Clause 5.7.3: the evolution of one entity on the live branch is its
    /// recorded rows and its queued entries, both read for that entity alone.
    #[test]
    fn the_history_read_unions_the_pending_rows_of_the_entity() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains(r#""re"."id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(sql.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(sql.contains(r#""pe"."entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        let snapshot = Alias::new("snap_7").into_iden();
        let recorded = build_temporal_history_query(&HistoryBranch::recorded_in(snapshot), &vehicle_id(), &minimal_compiled()).to_string(PostgresQueryBuilder);
        assert!(!recorded.contains("temporal_append"), "{recorded}");
    }

    /// `between` 2024-01-15T10:00Z and 2024-01-16T10:00Z, the window the page
    /// tests read.
    fn one_day() -> TimeInterval {
        TimeInterval::Between {
            from: "2024-01-15T10:00:00Z".parse().expect("valid instant"),
            to: "2024-01-16T10:00:00Z".parse().expect("valid instant"),
        }
    }

    /// The stage 2 statement of `page` over `interval` on the recorded rows
    /// alone.
    fn recorded_sql_of(page: InstancePage, interval: TimeInterval) -> String {
        let mut compiled = minimal_compiled();
        compiled.instances.page = page;
        compiled.window.interval = interval;
        build_temporal_history_query(&HistoryBranch::live_recorded(), &vehicle_id(), &compiled).to_string(PostgresQueryBuilder)
    }

    /// Clauses 5.7.3.4 and 5.7.4.4: `lastN` is the last `n` instances of each
    /// stream, read most recent first through the axis index and limited, so
    /// the read stops after them instead of ranking the window.
    #[test]
    fn a_last_n_page_reads_each_stream_backwards_with_a_limit() {
        let sql = recorded_sql_of(InstancePage::LastN { n: count(5) }, one_day());
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
        assert!(sql.contains(r#""rh"."stream_key" = "st"."stream_key""#), "{sql}");
        assert!(
            sql.contains(r#"ORDER BY "ah"."observed_at" DESC, "ah"."recorded_at" DESC, "ah"."write_id" DESC, "ah"."write_ordinal" DESC LIMIT 5"#),
            "{sql}"
        );
        assert!(!sql.contains("OVER"), "{sql}");
        assert!(sql.contains(r#"FALSE AS "stream_continues""#), "a complete answer reports no cut: {sql}");
    }

    /// Clause 6.3.10 forwards: the page is read through the axis index from
    /// the least recent instant, its edge the `size`-th instance, so neither
    /// read goes past the page.
    #[test]
    fn a_forward_page_reads_each_stream_with_a_limit() {
        let sql = recorded_sql_of(InstancePage::Forward { size: 100 }, one_day());
        assert!(sql.contains("LEFT JOIN LATERAL"), "{sql}");
        assert!(sql.contains(r#"ORDER BY "ah"."observed_at" ASC LIMIT 100"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "edge_candidates"."edge" ASC LIMIT 1 OFFSET 99"#), "{sql}");
        assert!(!sql.contains("OVER"), "{sql}");
        assert!(!sql.contains("COUNT(*)"), "{sql}");
    }

    /// The kept instances stay a subquery behind `OFFSET 0`, so the planner
    /// finds a stream's edge once before reading the stream's rows, whatever
    /// it estimates the stream to hold, rather than once per row.
    #[test]
    fn a_page_reads_its_kept_instances_behind_a_fence_after_the_edge() {
        for page in [
            InstancePage::Forward { size: 100 },
            InstancePage::Backward {
                size: 1000,
                requested: count(5000),
            },
        ] {
            let sql = recorded_sql_of(page, one_day());
            let edge = sql.find(r#"AS "stream_edge""#).expect("the stream edge lateral");
            let kept = sql.find(r#"OFFSET 0) AS "page""#).expect("the fenced page lateral");
            assert!(edge < kept, "{sql}");
        }
    }

    /// The page keeps every instance up to its edge instant rather than a
    /// count of them, so the group sharing the instant at the edge stays on
    /// one page; the instances on the inclusive lower bound are the group the
    /// previous page ended on and do not count towards the edge.
    #[test]
    fn instances_sharing_an_instant_stay_on_one_page() {
        let sql = recorded_sql_of(InstancePage::Forward { size: 100 }, one_day());
        assert!(
            sql.contains(r#""ah"."observed_at" <= COALESCE("stream_edge"."edge", 'infinity'::timestamptz)"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"AND "ah"."observed_at" > '2024-01-15 10:00:00.000000 +00:00' ORDER BY "ah"."observed_at" ASC LIMIT 100"#),
            "{sql}"
        );
        let unbounded = recorded_sql_of(InstancePage::Forward { size: 100 }, TimeInterval::Unbounded);
        assert!(!unbounded.contains(r#""ah"."observed_at" > '"#), "{unbounded}");
        assert!(
            unbounded.contains(r#"ORDER BY "ah"."observed_at" ASC LIMIT 100"#),
            "an unbounded window has no lower bound to leave uncounted: {unbounded}"
        );
    }

    /// Clause 6.3.10 backwards: `before` is an exclusive bound, so the page
    /// counts every instance and keeps all those from its edge on.
    #[test]
    fn a_backward_page_keeps_every_instance_from_its_edge() {
        let sql = recorded_sql_of(
            InstancePage::Backward {
                size: 1000,
                requested: count(5000),
            },
            TimeInterval::Before("2024-01-15T10:00:00Z".parse().expect("valid instant")),
        );
        assert!(sql.contains(r#"ORDER BY "ah"."observed_at" DESC LIMIT 1000"#), "{sql}");
        assert!(sql.contains("OFFSET 999"), "{sql}");
        assert!(
            sql.contains(r#""ah"."observed_at" >= COALESCE("stream_edge"."edge", '-infinity'::timestamptz)"#),
            "{sql}"
        );
    }

    /// A page reports whether each stream holds an instance beyond its edge,
    /// which is what the response's `Content-Range` is cut on.
    #[test]
    fn a_page_reports_whether_each_stream_runs_on_beyond_it() {
        let forward = recorded_sql_of(InstancePage::Forward { size: 100 }, one_day());
        assert!(forward.contains(r#"AS "stream_continues""#), "{forward}");
        assert!(forward.contains(r#""ah"."observed_at" > "edge_row"."edge""#), "{forward}");
        let backward = recorded_sql_of(
            InstancePage::Backward {
                size: 1000,
                requested: count(5000),
            },
            one_day(),
        );
        assert!(backward.contains(r#""ah"."observed_at" < "edge_row"."edge""#), "{backward}");
    }

    /// Every page shape a read compiles to, over a window unbounded on the
    /// axis.
    fn every_page() -> [InstancePage; 4] {
        [
            InstancePage::Whole,
            InstancePage::Forward { size: 100 },
            InstancePage::Backward {
                size: 1000,
                requested: count(5000),
            },
            InstancePage::LastN { n: count(3) },
        ]
    }

    /// Clause 4.5.7 places every instance of a temporal representation "at a
    /// particular point in time, which is recorded as a Temporal Property of
    /// the instance", so a read on any axis, whatever its page, represents
    /// only the instances that carry an instant there, and none is read apart.
    #[test]
    fn every_read_on_an_axis_represents_only_the_instances_carrying_it() {
        for (axis, column) in [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ] {
            for page in every_page() {
                let sql = recorded_sql_on(axis, page, TimeInterval::Unbounded);
                let carried = format!(r#""ah"."{column}" IS NOT NULL"#);
                let missing = format!(r#""ah"."{column}" IS NULL"#);
                assert!(sql.contains(&carried), "{column}: {sql}");
                assert!(!sql.contains(&missing), "{column}: {sql}");
            }
        }
    }

    /// Clause 4.5.19 places every aggregate in a period of the time range, the
    /// whole-range period of an absent `aggrPeriodDuration` included, so an
    /// aggregation of every instance or of the last `n`, over a grid or the
    /// whole range, folds only instances that carry an instant on the axis.
    #[test]
    fn every_aggregation_folds_only_the_instances_carrying_the_axis() {
        for period in ["PT1H", "PT0S"] {
            for page in every_page() {
                let mut compiled = minimal_compiled();
                compiled.representation.format = TemporalFormat::AggregatedValues;
                compiled.representation.aggr_methods = vec![AggrMethod::Sum];
                compiled.representation.aggr_period_duration = AggregationPeriod::parse(period).expect("valid period");
                compiled.instances.page = page;
                let sql = build_temporal_history_query(&HistoryBranch::live_recorded(), &vehicle_id(), &compiled).to_string(PostgresQueryBuilder);
                assert!(sql.contains(r#""ah"."observed_at" IS NOT NULL"#), "{period}: {sql}");
                assert!(!sql.contains(r#""ah"."observed_at" IS NULL"#), "{period}: {sql}");
            }
        }
    }

    /// On the live branch a stream's recorded rows and its queued entries are
    /// each ordered and limited before they are joined, so the limit reaches
    /// the axis index.
    #[test]
    fn each_part_of_a_live_stream_is_ordered_and_limited_on_its_own() {
        let mut compiled = minimal_compiled();
        compiled.instances.page = InstancePage::LastN { n: count(5) };
        compiled.window.interval = one_day();
        let sql = sql_of(&compiled);
        assert_eq!(sql.matches("LIMIT 5").count(), 3, "both parts and their join: {sql}");
        assert!(sql.contains(r#""rh"."stream_key" = "st"."stream_key""#), "{sql}");
        assert!(sql.contains(r#""pe"."attribute_name_key" = "st"."attribute_name_key""#), "{sql}");
        assert!(sql.contains(r#"("pe"."dataset_id" IS NOT DISTINCT FROM "st"."dataset_id")"#), "{sql}");
    }

    /// Every instance of a stream carries its attribute's name, so the name is
    /// joined once per stream rather than per instance read.
    #[test]
    fn a_page_joins_the_attribute_name_once_per_stream() {
        let sql = recorded_sql_of(InstancePage::Forward { size: 100 }, one_day());
        assert_eq!(sql.matches(r#"JOIN "attribute_names""#).count(), 1, "{sql}");
        assert!(
            sql.contains(r#"JOIN "attribute_names" AS "an" ON "an"."attribute_name_key" = "st"."attribute_name_key""#),
            "{sql}"
        );
    }

    /// The stage 2 statement of `page` over `interval` on the axis `axis`, on
    /// the recorded rows alone.
    fn recorded_sql_on(axis: TimeProp, page: InstancePage, interval: TimeInterval) -> String {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = axis;
        compiled.instances.page = page;
        compiled.window.interval = interval;
        build_temporal_history_query(&HistoryBranch::live_recorded(), &vehicle_id(), &compiled).to_string(PostgresQueryBuilder)
    }

    /// Clause 4.8: an appended instance is created and modified at its write
    /// instant, so a page on `createdAt` or `modifiedAt` orders and limits
    /// those rows on `recorded_at`, which the stream's write-clock index
    /// returns in order, and the rest apart; neither part leaves the page to
    /// the axis's BRIN index.
    #[test]
    fn a_system_time_page_reads_the_rows_stamped_at_their_write_instant_on_the_write_clock() {
        for (axis, column) in [(TimeProp::Created, "created_at"), (TimeProp::Modified, "modified_at")] {
            let sql = recorded_sql_on(axis, InstancePage::Forward { size: 100 }, one_day());
            let limited = format!(r#"ORDER BY "ah"."{column}" ASC LIMIT 100"#);
            assert_eq!(sql.matches(&limited).count(), 2, "an edge read per part: {sql}");
            let stamped = format!(r#""rh"."recorded_at" AS "{column}""#);
            let apart = format!(r#""rh"."{column}" IS DISTINCT FROM "rh"."recorded_at""#);
            assert_eq!(sql.matches(&stamped).count(), 3, "edge, continuation and kept reads: {sql}");
            assert_eq!(sql.matches(&apart).count(), 3, "edge, continuation and kept reads: {sql}");
            assert!(sql.contains(") OR EXISTS(SELECT 1 FROM"), "the stream continues in either part: {sql}");
        }
    }

    /// A `lastN` on a system-time axis takes the last `n` of each part and
    /// then the last `n` of both.
    #[test]
    fn a_system_time_last_n_takes_the_last_instances_of_each_part() {
        let sql = recorded_sql_on(TimeProp::Modified, InstancePage::LastN { n: count(5) }, one_day());
        assert_eq!(sql.matches("LIMIT 5").count(), 3, "both parts and their join: {sql}");
        assert!(sql.contains(r#""rh"."recorded_at" AS "modified_at""#), "{sql}");
        assert!(sql.contains(r#""rh"."modified_at" IS DISTINCT FROM "rh"."recorded_at""#), "{sql}");
    }

    /// `observedAt` is the client's instant, so a page on it reads each
    /// stream's recorded rows whole through the axis index.
    #[test]
    fn an_observed_page_reads_each_stream_in_one_recorded_part() {
        let sql = recorded_sql_on(TimeProp::Observed, InstancePage::Forward { size: 100 }, one_day());
        assert_eq!(sql.matches(r#"ORDER BY "ah"."observed_at" ASC LIMIT 100"#).count(), 1, "{sql}");
        assert!(!sql.contains("DISTINCT FROM"), "{sql}");
    }

    /// Every instance in the window is read when no page applies.
    #[test]
    fn the_whole_history_reads_every_instance() {
        let sql = sql_of(&minimal_compiled());
        assert!(!sql.contains("LATERAL"), "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }
}
