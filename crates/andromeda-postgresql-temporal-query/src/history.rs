//! Stage 2 of a temporal query: the attribute history of every entity of a
//! page, optionally narrowed to one clause 6.3.10 page of instances per
//! attribute, read in one statement.
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
    aggregation::{build_aggregation_query, in_listing_order},
    alias::{Ah, An},
    entity_reads::each_entity,
    stream_page::{Walk, in_rendering_order, last_n_history, project_whole_streams, walked_history},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_query::{
    dataset::DatasetFilter,
    temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, period_budget::PeriodBudget},
};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dataset_predicate::dataset_condition,
    dictionary::query_names::QueryNames,
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::attribute_rows,
        axis::{axis_column, axis_present, axis_window},
        grain::HistoryGrain,
    },
};
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::{pg_array::is_any_of, postgis::st_as_ewkb};
use iri_rs::IriBuf;
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};

/// Alias `entity_rows`: the rows the read of one Entity answers.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_rows"]
struct EntityRows;

/// Build Stage 2: the temporal history of each of `entity_ids`, every row
/// beside the identifier of its Entity, ordered by it and then in the order
/// the representation reads the rows of one Entity.
///
/// Every Entity is read on its own inside one statement
/// ([`each_entity`]), so a page of entities costs one round trip and one
/// planning, and each Entity's streams are still read through their own
/// indexes.
#[must_use]
pub fn build_temporal_history_query(
    branch: &HistoryBranch,
    entity_ids: &[Identifier],
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
) -> SelectStatement {
    // The ranked, unranked and aggregated shapes read the same entity's rows
    // among the same represented instances, so none can reach a row the others
    // cannot.
    let condition = represented_instances(compiled, names);
    let mut select = match compiled.representation.format {
        // The grouped extension format runs the same aggregation pipeline; the
        // builder adds the group-key columns when `representation.grouping` is set.
        TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => {
            let mut select = each_entity(entity_ids, EntityRows, |of| build_aggregation_query(branch, of, compiled, &condition, names));
            in_listing_order(&mut select, EntityRows);
            select
        }
        TemporalFormat::Normalized | TemporalFormat::TemporalValues => {
            let mut select = each_entity(entity_ids, EntityRows, |of| build_history_query(branch, of, compiled, condition));
            in_rendering_order(&mut select, EntityRows, axis_column(compiled.window.timeproperty));
            select
        }
    };
    select.expr(Expr::col((EntityRows, Asterisk)));
    select
}

/// The Attribute instances the temporal representation of an entity carries,
/// over the rows of [`base_history_source`]: stage 2 reads them, and stage 1
/// asks whether a projection leaves an entity any of them.
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
pub(crate) fn represented_instances(compiled: &CompiledTemporalQuery, names: &QueryNames) -> Condition {
    let timeproperty = compiled.window.timeproperty;
    let mut condition = Condition::all()
        .add(axis_present(Ah, timeproperty))
        .add(axis_window(Ah, timeproperty, &compiled.window.interval));

    // The `attrs` selection narrows by the dictionary keys its names resolved
    // to, on the source's own grain column.
    if !compiled.instances.attr_filter.is_empty() {
        let keys = names.attributes.keys_of(compiled.instances.attr_filter.iter().map(IriBuf::as_str));
        condition = condition.add(is_any_of(
            Expr::col((Ah, HistoryGrain::AttributeNameKey)),
            keys.into_iter().map(AttributeNameKey::get),
        ));
    }

    // Clause 5.7.4.4 step S8 (5.7.3.4 for a retrieval): the `datasetId`
    // parameter keeps only the instances of the datasets it names. It filters
    // the instances an Entity is represented with, never which Entities are.
    condition = condition.add(dataset_condition(
        &Expr::col((Ah, HistoryGrain::DatasetKey)),
        &DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter),
    ));

    // A Scope query selects Entities only (clauses 4.19 and 5.7.4.4 step S4):
    // the last step of clause 5.7.4.4 keeps the instances that meet "the
    // temporal, query and geoquery restrictions", so a selected Entity keeps
    // every in-window instance whatever Scope held when it was observed.
    condition
}

/// The attribute rows `of` reads beside the name of their attribute, shared by
/// the ranked and unranked shapes and the aggregation so none can reach a row
/// the others cannot.
pub(crate) fn base_history_source(select: &mut SelectStatement, branch: &HistoryBranch, of: &HistoryOf) {
    select.from(attribute_rows(branch, of, Ah));
    join_attribute_names(select);
}

/// Join the name of the attribute every row read under [`Ah`] keys, which
/// [`project_base_columns`] projects.
pub(crate) fn join_attribute_names(select: &mut SelectStatement) {
    select.join_as(
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
        AttributeHistory::ValueLanguage,
        AttributeHistory::ValueBoolean,
        AttributeHistory::ValueGeometryMembers,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueDateTime,
        AttributeHistory::ValueUri,
        AttributeHistory::UnitCodeId,
        AttributeHistory::ValueTypeKey,
        AttributeHistory::ValueDatatypeKey,
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
    select.expr(Expr::col((Ah, HistoryGrain::DatasetKey)));
}

/// Build the history query, narrowed to the page the query compiled; a page
/// of periods, which an instance representation never compiles to, counts its
/// periods as instances.
fn build_history_query(branch: &HistoryBranch, of: &HistoryOf, compiled: &CompiledTemporalQuery, condition: Condition) -> SelectStatement {
    match compiled.instances.page {
        InstancePage::Whole => whole_history(branch, of, condition),
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

/// Every instance in the window.
fn whole_history(branch: &HistoryBranch, of: &HistoryOf, condition: Condition) -> SelectStatement {
    let mut select = Query::select();
    project_base_columns(&mut select);
    project_whole_streams(&mut select);
    base_history_source(&mut select, branch, of);
    select.cond_where(condition);
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
        temporal::{
            aggregation_period::AggregationPeriod,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        history_source::HistoryBranch,
    };
    use iri_rs::IriBuf;
    use sea_query::{IntoIden, PostgresQueryBuilder};
    use std::{num::NonZeroU32, slice};

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
        build_temporal_history_query(&HistoryBranch::live(), slice::from_ref(&vehicle_id()), compiled, &QueryNames::default()).to_string(PostgresQueryBuilder)
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
    /// deleted, so stage 2 reaches the entity grain through the stream's
    /// identity row and not through the current-state tables.
    #[test]
    fn history_reaches_the_entity_through_the_temporal_catalogue() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains("JOIN \"temporal_attribute_streams\" AS \"rs\" ON \"rs\".\"stream_key\" = \"rh\".\"stream_key\""),
            "{sql}"
        );
        assert!(
            sql.contains("JOIN \"entities\" AS \"re\" ON \"re\".\"entity_key\" = \"rs\".\"entity_key\""),
            "{sql}"
        );
        for current_state in ["\"entity_search\"", "\"current_attributes\""] {
            assert!(!sql.contains(current_state), "{current_state}: {sql}");
        }
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
    /// `recorded_at` is projected for ordering only, never as a substitute for
    /// an axis the row does not carry: no axis is coalesced with another.
    #[test]
    fn all_four_time_axes_are_projected_without_substitution() {
        let sql = sql_of(&minimal_compiled());
        for column in ["observed_at", "created_at", "modified_at", "deleted_at", "recorded_at"] {
            assert!(sql.contains(&format!("\"ah\".\"{column}\"")), "{column} missing: {sql}");
        }
        assert!(!sql.contains("COALESCE(\"ah\""), "{sql}");
        assert!(sql.contains(r#""ah"."observed_at" IS NOT NULL"#), "{sql}");
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
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(
                "https://uri.etsi.org/ngsi-ld/default-context/speed".parse().expect("valid attribute name"),
                AttributeNameKey::from(7),
            )]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        };
        let sql =
            build_temporal_history_query(&HistoryBranch::live_recorded(), slice::from_ref(&vehicle_id()), &compiled, &names).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""ah"."attribute_name_key" = 7"#), "the attrs narrow by dictionary key: {sql}");
        assert!(!sql.contains(r#""an"."name" ="#), "{sql}");
    }

    /// Clause 4.5.5: `@none` selects the instance that carries no `datasetId`,
    /// which the stream catalogue holds as a NULL column and its grain index
    /// keys as the empty string, so `@none` beside named datasets is one
    /// membership test.
    #[test]
    fn dataset_id_none_selects_the_default_instance() {
        let selectors = |raw: &[&str]| -> Vec<DatasetSelector> { raw.iter().map(|entry| DatasetSelector::parse(entry).expect("valid")).collect() };

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains(r#""ah"."dataset_key" = ('')"#), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:Dataset"), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains(r#""ah"."dataset_key" = 'urn:ngsi-ld:Dataset:one'"#), "{sql}");

        let mut compiled = minimal_compiled();
        compiled.instances.dataset_id_filter = selectors(&["@none", "urn:ngsi-ld:Dataset:one"]);
        let sql = sql_of(&compiled);
        assert!(sql.contains(r#""ah"."dataset_key" = ANY(ARRAY ['urn:ngsi-ld:Dataset:one',''])"#), "{sql}");
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

    /// Clause 5.7.4.4 keeps the instances that meet the temporal, query and
    /// geoquery restrictions, so a Scope query, which stage 1 applies to
    /// select the Entity, leaves the instances of the Entity whole on every
    /// axis.
    #[test]
    fn a_temporal_scope_q_leaves_the_instances_of_a_selected_entity_whole() {
        for axis in [TimeProp::Observed, TimeProp::Modified] {
            let mut scoped = minimal_compiled();
            scoped.window.timeproperty = axis;
            scoped.entities.scope_q = Some(ScopeQuery::parse("/Madrid/#").expect("valid scope query"));
            let mut unscoped = minimal_compiled();
            unscoped.window.timeproperty = axis;
            let sql = sql_of(&scoped);
            assert_eq!(sql, sql_of(&unscoped), "{axis}");
            assert!(!sql.contains("scope_history"), "{axis}: {sql}");
        }
    }

    /// Clause 5.7.3: the evolution of one entity on the live branch is its
    /// recorded rows and its queued entries, both read for that entity alone.
    #[test]
    fn the_history_read_unions_the_pending_rows_of_the_entity() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains(r#"UNNEST(ARRAY[ 'urn:ngsi-ld:Vehicle:1' ]) AS "read_entity""#), "{sql}");
        assert!(sql.contains(r#""re"."id" = "read_entity"."read_entity""#), "{sql}");
        assert!(sql.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(sql.contains(r#""pe"."entity_id" = "read_entity"."read_entity""#), "{sql}");
        let snapshot = Alias::new("snap_7").into_iden();
        let recorded = build_temporal_history_query(
            &HistoryBranch::recorded_in(snapshot),
            slice::from_ref(&vehicle_id()),
            &minimal_compiled(),
            &QueryNames::default(),
        )
        .to_string(PostgresQueryBuilder);
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
        build_temporal_history_query(
            &HistoryBranch::live_recorded(),
            slice::from_ref(&vehicle_id()),
            &compiled,
            &QueryNames::default(),
        )
        .to_string(PostgresQueryBuilder)
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
            sql.contains(r#"ORDER BY "ah"."observed_at" DESC, "ah"."write_id" DESC, "ah"."write_ordinal" DESC LIMIT 5"#),
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
            sql.contains(concat!(
                r#"AND "ah"."observed_at" > '2024-01-15 10:00:00.000000 +00:00' "#,
                r#"ORDER BY "ah"."observed_at" ASC LIMIT 100"#
            )),
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
                let sql = build_temporal_history_query(
                    &HistoryBranch::live_recorded(),
                    slice::from_ref(&vehicle_id()),
                    &compiled,
                    &QueryNames::default(),
                )
                .to_string(PostgresQueryBuilder);
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
        assert!(sql.contains(r#""pe"."dataset_key" = "st"."dataset_key""#), "{sql}");
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
        build_temporal_history_query(
            &HistoryBranch::live_recorded(),
            slice::from_ref(&vehicle_id()),
            &compiled,
            &QueryNames::default(),
        )
        .to_string(PostgresQueryBuilder)
    }

    /// Clause 4.8 has the broker stamp `createdAt` and `modifiedAt` on every
    /// instance, so a page on either reads each stream's recorded rows in one
    /// part, ordered and bounded on the axis column, which its stream-led
    /// index keys.
    #[test]
    fn a_system_time_page_reads_one_recorded_part_ordered_on_the_axis_column() {
        for (axis, column) in [(TimeProp::Created, "created_at"), (TimeProp::Modified, "modified_at")] {
            let sql = recorded_sql_on(axis, InstancePage::Forward { size: 100 }, one_day());
            let ordered = format!(r#"ORDER BY "ah"."{column}" ASC LIMIT 100"#);
            assert_eq!(sql.matches(&ordered).count(), 1, "one edge read: {sql}");
            assert!(sql.contains(&format!(r#""ah"."{column}" >= '"#)), "{sql}");
            assert!(sql.contains(&format!(r#""ah"."{column}" <= COALESCE("stream_edge"."edge""#)), "{sql}");
            assert!(!sql.contains("DISTINCT FROM") && !sql.contains(r#""rh"."recorded_at" AS"#), "{sql}");
            assert!(!sql.contains(") OR EXISTS(SELECT 1 FROM"), "one part to continue in: {sql}");
        }
    }

    /// A `lastN` on a system-time axis takes the last `n` of the recorded
    /// part on the axis column, and the last `n` of the parts joined on the
    /// instant they project.
    #[test]
    fn a_system_time_last_n_takes_the_last_instances_on_the_axis_column() {
        let sql = recorded_sql_on(TimeProp::Modified, InstancePage::LastN { n: count(5) }, one_day());
        assert_eq!(sql.matches("LIMIT 5").count(), 2, "the part and the join: {sql}");
        assert!(sql.contains(r#"ORDER BY "ah"."modified_at" DESC, "ah"."write_id" DESC"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "latest"."modified_at" DESC, "latest"."write_id" DESC"#), "{sql}");
    }

    /// `observedAt` is the client's instant, so a page on it reads each
    /// stream's recorded rows whole through the axis index.
    #[test]
    fn an_observed_page_reads_each_stream_in_one_recorded_part() {
        let sql = recorded_sql_on(TimeProp::Observed, InstancePage::Forward { size: 100 }, one_day());
        assert_eq!(sql.matches(r#"ORDER BY "ah"."observed_at" ASC LIMIT 100"#).count(), 1, "{sql}");
        assert!(!sql.contains("DISTINCT FROM"), "{sql}");
    }

    /// Every instance in the window is read when no page applies: the one
    /// LATERAL is the read of each entity, none reads a stream on its own.
    #[test]
    fn the_whole_history_reads_every_instance() {
        let sql = sql_of(&minimal_compiled());
        pretty_assertions::assert_eq!(sql.matches("LATERAL").count(), 1, "{sql}");
        assert!(!sql.contains("LIMIT"), "{sql}");
    }
}
