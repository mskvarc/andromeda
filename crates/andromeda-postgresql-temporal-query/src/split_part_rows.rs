//! The rows of the part of each contributed entity this broker holds that a
//! split temporal query judges in memory, beside the parts the Context Sources
//! answered (clauses 4.11 and 5.7.4.4).
//!
//! The Scope in force when a window opens is the latest Scope event before the
//! start (clause 4.18) across every part of a split entity, so no part decides
//! it alone: each part answers the event it held, and the parts are merged
//! before the scope filter judges the survivor. A removal is answered as well,
//! because a newer removal in one part ends what an older event of another
//! part held.
//!
//! A Query Term crossing a Relationship hop reaches entities any part may
//! hold, so the local instances inside the window of the Relationships its
//! hops start from are read as rows too, and the term is judged in memory
//! over the targets of every part.

use crate::{
    alias::Ah,
    entity_reads::READ_ENTITY_ID,
    history::{join_attribute_names, project_base_columns},
    scope_history::{order_scope_events, project_scope_event},
    stream_page::project_whole_streams,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, window::TimeInterval};
use andromeda_postgresql_query::{
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::attribute_rows,
        axis::{axis_present, axis_window},
        entity_rows::entity_rows,
        scope_rows::scope_rows,
    },
    scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis},
    window_reach::WindowReach,
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::pg_array::is_any_of;
use andromeda_postgresql_stored_form::history_grain::HistoryGrain;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Alias, Asterisk, Expr as SeaExpr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement, SimpleExpr};

/// Build the statement reading, for every entity of `entity_ids` this broker
/// holds a Temporal Evolution of, the rows of the Scope event in force when
/// the window opens, as scope history rows beside the entity's identifier,
/// ordered by it; `None` when the window has no Scope in effect before it.
///
/// The event in force is the one before the start that no later event
/// superseded at or before it (clause 4.18): an event at the start itself lies
/// inside the window.
#[must_use]
pub fn build_in_effect_scope(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, entity_ids: &[Identifier]) -> Option<SelectStatement> {
    let start = compiled.window.in_effect_start()?;
    let axis = scope_history_axis(compiled.window.timeproperty);
    let of = HistoryOf::Entity(SeaExpr::col((E, Entities::Id)));
    let mut events = Query::select();
    events
        .column((ScopeEvent, Asterisk))
        .from(scope_rows(branch, &of, ScopeEvent))
        .and_where(positioned_on_axis(axis))
        .and_where(SeaExpr::col((ScopeEvent, axis)).lt(start))
        .cond_where(overlaps_window(axis, TimeInterval::After(start), WindowReach::InsideAndInEffect, branch, &of));
    let mut select = Query::select();
    project_scope_event(&mut select, axis);
    select
        .expr_as(SeaExpr::col((E, Entities::Id)), Alias::new(READ_ENTITY_ID))
        .from(entity_rows(branch, E))
        .join_lateral(JoinType::Join, events.take(), ScopeEvent, SimpleExpr::from(true))
        .and_where(of_entities(entity_ids))
        .order_by((E, Entities::Id), Order::Asc);
    order_scope_events(&mut select, axis);
    Some(select.take())
}

/// Build the statement reading, for every entity of `entity_ids` this broker
/// holds a Temporal Evolution of, its instances inside the window of the
/// Relationships keyed `relationships`, as history rows beside the entity's
/// identifier, ordered by it.
///
/// An instance carrying no instant on the window's axis lies in no window
/// (clause 4.5.7), an unbounded one included.
#[must_use]
pub fn build_window_relationship_instances(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    entity_ids: &[Identifier],
    relationships: &[AttributeNameKey],
) -> SelectStatement {
    let of = HistoryOf::Entity(SeaExpr::col((E, Entities::Id)));
    let timeproperty = compiled.window.timeproperty;
    let mut rows = Query::select();
    rows.column((WindowRow, Asterisk))
        .from(attribute_rows(branch, &of, WindowRow))
        .and_where(is_any_of(
            SeaExpr::col((WindowRow, HistoryGrain::AttributeNameKey)),
            relationships.iter().map(|key| key.get()),
        ))
        .cond_where(axis_present(WindowRow, timeproperty))
        .cond_where(axis_window(WindowRow, timeproperty, &compiled.window.interval));
    let mut select = Query::select();
    project_base_columns(&mut select);
    project_whole_streams(&mut select);
    select
        .expr_as(SeaExpr::col((E, Entities::Id)), Alias::new(READ_ENTITY_ID))
        .from(entity_rows(branch, E))
        .join_lateral(JoinType::Join, rows.take(), Ah, SimpleExpr::from(true));
    join_attribute_names(&mut select);
    select.and_where(of_entities(entity_ids)).order_by((E, Entities::Id), Order::Asc);
    select.take()
}

/// The alias the window's Relationship rows are read under inside the lateral
/// source.
#[derive(Iden, Clone, Copy)]
#[iden = "wr"]
struct WindowRow;

/// The temporal entity `e` names is one of `entity_ids`.
fn of_entities(entity_ids: &[Identifier]) -> SimpleExpr {
    is_any_of(SeaExpr::col((E, Entities::Id)), entity_ids.iter().map(Identifier::to_string))
}

#[cfg(test)]
mod tests {
    use crate::split_part_rows::{build_in_effect_scope, build_window_relationship_instances};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;

    fn compiled(interval: TimeInterval) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).interval(interval).build())
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn noon() -> TimeInterval {
        TimeInterval::After("2020-08-01T12:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get())
    }

    fn ids() -> Vec<Identifier> {
        vec!["urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")]
    }

    /// Clause 4.18: the Scope in force at the start is the latest event before
    /// it that nothing superseded at or before it.
    #[test]
    fn the_in_effect_scope_is_the_latest_event_before_the_start() {
        let statement = build_in_effect_scope(&HistoryBranch::live_recorded(), &compiled(noon()), &ids()).expect("a window with a start");
        assert_statement_snapshot!("split-part-rows-the-in-effect-scope-is-the-latest-event-before-the-start", statement);
    }

    /// Clause 4.5.7: the hop Relationships are read inside the window only,
    /// every instance placed on the axis.
    #[test]
    fn the_window_relationships_are_read_inside_the_window() {
        let statement = build_window_relationship_instances(&HistoryBranch::live_recorded(), &compiled(noon()), &ids(), &[AttributeNameKey::from(4)]);
        assert_statement_snapshot!("split-part-rows-the-window-relationships-are-read-inside-the-window", statement);
    }

    /// A window with no start has no Scope in effect to find.
    #[test]
    fn a_window_with_no_start_reads_no_scope_in_effect() {
        assert!(build_in_effect_scope(&HistoryBranch::live(), &compiled(TimeInterval::Unbounded), &ids()).is_none());
    }
}
