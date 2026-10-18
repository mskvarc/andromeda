//! The scope half of stage 2: the reified temporal scope of one entity.
//!
//! Clause 4.18 makes the Scope "a special scope Property that is non-reified in
//! the normalized Entity representation and reified in the temporal
//! representation of an Entity", and clause 4.5.6 has that reified form carry
//! the four non-reified Temporal Properties. It is stored apart from attribute
//! history: `scope_history` hangs off the temporal entity and has no attribute
//! stream, because clause 4.5.4 gives a Scope no dataset dimension and no
//! attribute family, so the temporal representation is assembled from two
//! statements rather than one, and this is the second of them.
//!
//! What it selects is not the events inside the window. Clause 4.18 makes a
//! Scope "valid from the time it has been set until the time it has been
//! explicitly removed", so the statement selects every event whose validity
//! interval overlaps the window, which is what carries annex C clause C.5.16's
//! Vehicle B9211 (scoped at 11:00, queried between 12:00 and 13:00) into the
//! answer with the 11:00 instant the annex renders.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_query::{
    history_source::{HistoryBranch, HistoryOf, scope_rows::scope_rows},
    scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis},
};
use andromeda_postgresql_schema::temporal::ScopeHistory;
use sea_query::{Alias, Expr, Order, Query, SelectStatement};

/// The name the statement projects the requested axis under.
///
/// It is the instant that identifies one scope event, and the store folds the
/// rows sharing it into one instance. Projecting it under a fixed name is what
/// lets the store group without having to be told which of the four Temporal
/// Properties clause 5.2.21 the request selected.
pub const SCOPE_EVENT_AT: &str = "scope_event_at";

/// Whether the request's `attrs` selection reaches the reified scope at all.
///
/// Table 6.18.3.2-1 makes `attrs` "a synonym for a combination of the pick and
/// q parameters", and `pick` names Entity members ("id", "type", "scope" or an
/// Attribute name), so a temporal query may name `scope` there; annex C clause
/// C.5.16 does exactly that with `attrs=speed,scope`. An `attrs` that names
/// other members and not `scope` therefore excludes it, and an absent `attrs`
/// selects everything the temporal representation carries.
#[must_use]
pub fn scope_is_selected(compiled: &CompiledTemporalQuery) -> bool {
    compiled.instances.attr_filter.is_empty()
        || compiled
            .instances
            .attr_filter
            .iter()
            .any(|attribute| attribute.as_str() == expanded_properties::SCOPE.as_str())
}

/// Build the scope half of stage 2: the scope events of one entity whose
/// clause 4.18 validity interval overlaps the compiled window.
///
/// Every row is returned, ordered so that the rows of a single event arrive
/// together: clause 4.18 admits at most one instance of the scope Property per
/// Entity and renders more than one Scope as a JSON array, so the several rows
/// a multi-Scope write lands at one instant are one rendered instance. One
/// write can still state several instances: clause 4.5.7 makes the temporal
/// representation of a Property "an Array of JSON-LD objects, each one
/// representing an instance of the referred Property [...] at a particular
/// point in time", so the instant separates the events and the write identity
/// alone does not.
///
/// `lastN` does not bound this statement. Clause 5.7.4.4 defines it over "a
/// number, n, of Attribute instances", and clause 4.5.6 names the Scope beside
/// "each Property and Relationship" rather than among them; the events the
/// window admits are what the representation carries.
#[must_use]
pub fn build_scope_history_query(branch: &HistoryBranch, entity_id: &Identifier, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let axis = scope_history_axis(compiled.window.timeproperty);

    let mut select = Query::select();
    for column in [
        ScopeHistory::RecordedAt,
        ScopeHistory::WriteId,
        ScopeHistory::WriteOrdinal,
        ScopeHistory::ValueKind,
        ScopeHistory::ObservedAt,
        ScopeHistory::CreatedAt,
        ScopeHistory::ModifiedAt,
        ScopeHistory::DeletedAt,
        // The Scope as it was written: the `ltree` label path beside it forgets
        // the optional leading `/` of clause 4.18, and clause 4.6.4 has the
        // broker "return the original content".
        ScopeHistory::VerbatimScopePath,
    ] {
        select.expr(Expr::col((ScopeEvent, column)));
    }
    // The requested axis under a fixed name: it is what separates one scope
    // event from the next, and clause 4.5.7 lets one write state several
    // instances of a Property "at a particular point in time" each.
    select.expr_as(Expr::col((ScopeEvent, axis)), Alias::new(SCOPE_EVENT_AT));

    // Clause 5.6.6 leaves the Temporal Evolution standing when the entity is
    // deleted, so the scope source reads the identity from the temporal
    // catalogue and never from current state.
    select
        .from(scope_rows(branch, &HistoryOf::Entity(Expr::val(entity_id.as_str())), ScopeEvent))
        .cond_where(positioned_on_axis(axis))
        .cond_where(overlaps_window(axis, compiled.window.interval, branch))
        .order_by((ScopeEvent, axis), Order::Asc)
        .order_by((ScopeEvent, ScopeHistory::RecordedAt), Order::Asc)
        .order_by((ScopeEvent, ScopeHistory::WriteId), Order::Asc)
        .order_by((ScopeEvent, ScopeHistory::WriteOrdinal), Order::Asc);

    select.take()
}

#[cfg(test)]
mod tests {
    use crate::scope_history::{build_scope_history_query, scope_is_selected};
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
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
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use iri_rs::IriBuf;
    use sea_query::PostgresQueryBuilder;

    fn vehicle_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:B9211".parse().expect("valid entity id")
    }

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

    fn sql_of(compiled: &CompiledTemporalQuery) -> String {
        build_scope_history_query(&HistoryBranch::live(), &vehicle_id(), compiled).to_string(PostgresQueryBuilder)
    }

    fn iri(raw: &str) -> IriBuf {
        IriBuf::new(raw.to_string()).expect("valid IRI")
    }

    /// Clause 5.6.6 leaves the evolution standing when the entity is deleted,
    /// so the statement reaches the entity through the temporal catalogue and
    /// names neither `entities` nor an attribute stream a Scope never has.
    #[test]
    fn the_scope_read_reaches_the_entity_through_the_temporal_catalogue() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains("JOIN \"temporal_entities\" AS \"re\" ON \"re\".\"temporal_entity_key\" = \"rsh\".\"temporal_entity_key\""),
            "{sql}"
        );
        assert!(!sql.contains("\"entities\""), "{sql}");
        assert!(!sql.contains("temporal_attribute_streams"), "{sql}");
    }

    /// Clause 4.5.6 gives the reified scope all four Temporal Properties, so
    /// each is projected from its own column and the operational write clock is
    /// projected for ordering alone.
    #[test]
    fn all_four_axes_are_projected_beside_the_write_identity() {
        let sql = sql_of(&minimal_compiled());
        for column in ["observed_at", "created_at", "modified_at", "deleted_at", "recorded_at"] {
            assert!(sql.contains(&format!("\"sh\".\"{column}\"")), "{column} missing: {sql}");
        }
        assert!(sql.contains("\"sh\".\"write_id\""), "{sql}");
        assert!(sql.contains("\"sh\".\"write_ordinal\""), "{sql}");
    }

    /// The Scope is read back from the text it was written in, never rebuilt
    /// from its label path.
    #[test]
    fn the_written_scope_is_projected() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"sh\".\"verbatim_scope_path\""), "{sql}");
        assert!(!sql.contains("\"scope_path\"::text"), "{sql}");
    }

    /// Clause 4.18: a Scope set before the window and never removed is valid
    /// throughout it. Annex C clause C.5.16 renders exactly that instant for
    /// Vehicle B9211, so the window must not be a membership test on the event.
    #[test]
    fn a_between_window_carries_forward_the_scope_that_was_already_valid() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::Between {
            from: "2018-08-01T12:00:00Z".parse().expect("valid instant"),
            to: "2018-08-01T13:00:00Z".parse().expect("valid instant"),
        };
        let sql = sql_of(&compiled);
        assert!(sql.contains("\"sh\".\"observed_at\" < "), "{sql}");
        assert!(!sql.contains("\"sh\".\"observed_at\" >= "), "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    /// The rows of one event arrive together and in write order, which is what
    /// lets the store fold a multi-Scope write into the single instance clause
    /// 4.18 allows an Entity.
    #[test]
    fn the_rows_of_one_event_are_ordered_together() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains("ORDER BY \"sh\".\"observed_at\" ASC, \"sh\".\"recorded_at\" ASC, \"sh\".\"write_id\" ASC, \"sh\".\"write_ordinal\" ASC"),
            "{sql}"
        );
    }

    /// Clause 4.5.7 lets one write state several instances of a Property, each
    /// "at a particular point in time", so the instant that separates two scope
    /// events is projected under a fixed name for the store to group on.
    #[test]
    fn the_requested_axis_is_projected_as_the_event_instant() {
        let sql = sql_of(&minimal_compiled());
        assert!(sql.contains("\"sh\".\"observed_at\" AS \"scope_event_at\""), "{sql}");

        let mut on_modified = minimal_compiled();
        on_modified.window.timeproperty = TimeProp::Modified;
        let sql = sql_of(&on_modified);
        assert!(sql.contains("\"sh\".\"modified_at\" AS \"scope_event_at\""), "{sql}");
    }

    /// Clause 5.2.21: the requested `timeproperty` positions the scope event,
    /// exactly as it positions an attribute instance.
    #[test]
    fn each_time_property_positions_the_scope_read_on_its_own_column() {
        for (timeproperty, column) in [
            (TimeProp::Observed, "observed_at"),
            (TimeProp::Created, "created_at"),
            (TimeProp::Modified, "modified_at"),
            (TimeProp::Deleted, "deleted_at"),
        ] {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = timeproperty;
            let sql = sql_of(&compiled);
            assert!(sql.contains(&format!("\"sh\".\"{column}\" IS NOT NULL")), "{timeproperty}: {sql}");
        }
    }

    /// Table 6.18.3.2-1 makes `attrs` a synonym for `pick`, which names Entity
    /// members, so annex C clause C.5.16's `attrs=speed,scope` reaches the
    /// scope while an `attrs` naming other members alone does not.
    #[test]
    fn the_attrs_selection_decides_whether_the_scope_is_read() {
        let mut unfiltered = minimal_compiled();
        assert!(scope_is_selected(&unfiltered));

        unfiltered.instances.attr_filter = vec![iri("https://uri.etsi.org/ngsi-ld/default-context/speed")];
        assert!(!scope_is_selected(&unfiltered));

        unfiltered.instances.attr_filter = vec![
            iri("https://uri.etsi.org/ngsi-ld/default-context/speed"),
            iri(expanded_properties::SCOPE.as_str()),
        ];
        assert!(scope_is_selected(&unfiltered));
    }

    /// Clause 4.18: the reified Scope of the live branch includes the Scope
    /// entries still queued for the entity.
    #[test]
    fn the_scope_history_read_unions_pending_scope_entries() {
        let sql = sql_of(&minimal_compiled());
        assert!(
            sql.contains(r#"FROM "temporal_append_entries" AS "pe" WHERE "pe"."entry_kind" IN (3, 4)"#),
            "{sql}"
        );
        assert!(sql.contains(&format!(r#""pe"."entity_id" = '{}'"#, vehicle_id())), "{sql}");
    }
}
