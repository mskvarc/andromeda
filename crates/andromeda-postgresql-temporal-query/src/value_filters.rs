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

use crate::{
    alias::{Ahg, Ang},
    axis::axis_column,
};
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    error::RenderError,
    geo_predicate::relation_expression,
    history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, grain::HistoryGrain},
    q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection as q_filter},
    scope_predicate::{ScopeTarget, condition as scope_condition},
    time_interval_predicate::within_interval,
};
use andromeda_postgresql_schema::{
    current::E,
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalEntities},
};
use andromeda_postgresql_seaquery::{cast::cast, literal::int_literal, pg_type::PostgresType};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, JoinType, SelectStatement};

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
    let mut filters = Condition::all();
    let mut present = false;
    if let Some(q_expr) = compiled.entities.q_expr.as_ref() {
        filters = filters.add(q_filter::compile(q_expr, &InstanceScan::History(&windowed_instances(branch, compiled)), names));
        present = true;
    }
    if let Some(probe) = temporal_geo_existential(branch, compiled)? {
        filters = filters.add(SeaExpr::exists(probe));
        present = true;
    }
    if let Some(scope_q) = &compiled.entities.scope_q {
        filters = filters.add(scope_condition(scope_q, &scope_history_target(branch, compiled)));
        present = true;
    }
    Ok(present.then(|| exempting_contributed(filters, &compiled.contributed)))
}

/// `filters`, or an entity a Context Source contributed.
fn exempting_contributed(filters: Condition, contributed: &ContributedEntities) -> Condition {
    if contributed.is_empty() {
        return filters;
    }
    let ids: Vec<&str> = contributed.as_slice().iter().map(AsRef::as_ref).collect();
    Condition::any().add(filters).add(SeaExpr::col((E, TemporalEntities::Id)).is_in(ids))
}

/// The scope-history target a temporal `scopeQ` is read against: a validity
/// interval of the entity `e` names overlapping the query's window on its
/// Temporal Property (clause 4.18).
pub(crate) fn scope_history_target(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> ScopeTarget {
    ScopeTarget::history(
        (E, TemporalEntities::Id),
        compiled.window.timeproperty,
        compiled.window.interval,
        branch.clone(), // clone: the target owns the branch its probe reads
    )
}

/// The Attribute instances a temporal `q` is checked against (clause 5.7.4.4
/// step S2): the entity's history rows valid within the window on the
/// requested Temporal Property, of every `datasetId`.
pub(crate) fn windowed_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> HistoryWindow {
    HistoryWindow::builder()
        .branch(branch.clone()) // clone: the window owns the branch its probes read
        .entity_id(SeaExpr::col((E, TemporalEntities::Id)))
        .axis(axis_column(compiled.window.timeproperty))
        .interval(compiled.window.interval)
        .build()
}

/// The correlated `EXISTS` over the `attribute_history` rows a temporal geo
/// query (clause 4.10) filters, restricted to the temporal window.
///
/// Clause 5.7.4.4 step S3: the geospatial restriction "shall be checked against
/// the `GeoProperty` instances that are within the interval defined by the
/// temporal query". Every instance of the target `GeoProperty` - the default
/// `location` and any named one alike - is an `attribute_history` row carrying a
/// `value_geometry`, so the probe reaches them uniformly through the history
/// rows rather than through the single hoisted current-state `location` column, which holds
/// only one instant and answers a different question. Clause 4.10 makes one
/// in-window instance meeting the restriction enough, which is exactly this
/// existential. `None` when the query carries no geo restriction.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] or [`RenderError::Query`]
/// exactly as the shared relation rendering does.
pub(crate) fn temporal_geo_existential(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> Result<Option<SelectStatement>, RenderError> {
    let Some(geo) = &compiled.entities.geo else {
        return Ok(None);
    };

    let planar = SeaExpr::col((Ahg, AttributeHistory::ValueGeometry));
    let geographic = cast(SeaExpr::col((Ahg, AttributeHistory::ValueGeometry)), PostgresType::Geography);
    let spatial = relation_expression(&geo.georel, planar, geographic, &geo.geometry)?;

    let mut probe = SelectStatement::new();
    probe
        .expr(int_literal(1))
        .from(attribute_rows(branch, &HistoryOf::Entity(SeaExpr::col((E, TemporalEntities::Id))), Ahg))
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            Ang,
            SeaExpr::col((Ang, AttributeNames::AttributeNameKey)).equals((Ahg, HistoryGrain::AttributeNameKey)),
        )
        .cond_where(SeaExpr::col((Ang, AttributeNames::Name)).eq(geo.geoproperty.as_str()))
        .cond_where(SeaExpr::col((Ahg, AttributeHistory::ValueGeometry)).is_not_null())
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
    within_interval(&SeaExpr::col((Ahg, axis)), &compiled.window.interval)
}
