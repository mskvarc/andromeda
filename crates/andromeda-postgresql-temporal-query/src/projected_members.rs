//! Stage 1's reading of a clause 4.21 projection as a selection criterion.
//!
//! Clause 5.7.4.5 reduces "every Entity within the payload body" to the Entity
//! members the projection defines, and an Entity it would reduce to no member at
//! all is not selected (`ProjectedMembers`). What the temporal representation of
//! an Entity carries is decided by the steps of clause 5.7.4.4 that precede the
//! projection: the window and the `attrs` of step S1 and the `datasetId` of step
//! S8, together with the clause 4.18 validity a `scopeQ` bounds the instances
//! by. So the Attribute half of the criterion asks whether any instance stage 2
//! would represent belongs to an Attribute the projection keeps, reusing the
//! very filter stage 2 reads its rows with; and the `scope` half, when the
//! projection keeps `scope`, whether any scope event stage 2 would represent
//! overlaps the window.

use crate::{
    alias::An,
    history::{base_history_source, represented_instances},
    scope_history::scope_is_selected,
};
use andromeda_ngsi_ld_query::{
    projection::projected_members::{ProjectedMembers, ScopeRetention},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::{
    history_source::{HistoryBranch, HistoryOf, scope_rows::scope_rows},
    scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis},
};
use andromeda_postgresql_schema::{current::E, name_dictionary::AttributeNames, temporal::TemporalEntities};
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Condition, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The condition an entity of the stage 1 `e` relation has to satisfy for the
/// query's projection to leave it a member; `None` when every entity does.
#[must_use]
pub fn projected_member_condition(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> Option<Condition> {
    let projected = &compiled.entities.projected_members;
    let attribute_name = Expr::col((An, AttributeNames::Name));
    let (kept_names, scope) = match projected {
        ProjectedMembers::Unconditional => return None,
        ProjectedMembers::AnyOf { scope, .. } => (attribute_name.is_in(projected.names()), *scope),
        ProjectedMembers::AnyBesides { scope, .. } => (attribute_name.is_not_in(projected.names()), *scope),
    };
    let attribute = Expr::exists(represented_attribute(branch, compiled, kept_names));
    Some(match scope {
        ScopeRetention::Kept if scope_is_selected(compiled) => Condition::any().add(attribute).add(Expr::exists(represented_scope(branch, compiled))),
        ScopeRetention::Kept | ScopeRetention::Removed => Condition::all().add(attribute),
    })
}

/// The instances of the entity stage 2 would represent, restricted to the
/// Attributes whose names satisfy `kept_names`.
fn represented_attribute(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, kept_names: SimpleExpr) -> SelectStatement {
    let mut probe = Query::select();
    probe.expr(int_literal(1));
    base_history_source(&mut probe, branch, &of_selected_entity());
    probe
        .cond_where(represented_instances(branch, &of_selected_entity(), compiled))
        .cond_where(kept_names);
    probe.take()
}

/// The scope events of the entity whose clause 4.18 validity overlaps the
/// window, which is what the scope half of stage 2 represents.
fn represented_scope(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let axis = scope_history_axis(compiled.window.timeproperty);
    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from(scope_rows(branch, &of_selected_entity(), ScopeEvent))
        .cond_where(positioned_on_axis(axis))
        .cond_where(overlaps_window(axis, compiled.window.interval, branch, &of_selected_entity()));
    probe.take()
}

/// The history of the entity of the stage 1 `e` relation.
fn of_selected_entity() -> HistoryOf {
    HistoryOf::Entity(Expr::col((E, TemporalEntities::Id)))
}

#[cfg(test)]
mod tests {
    use crate::projected_members::projected_member_condition;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        entity_selection::EntitySelection,
        projection::{compiled::Projection, projected_members::ProjectedMembers, term::ProjectionTerm},
        temporal::{
            compiled::CompiledTemporalQuery,
            instance_selection::InstanceSelection,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use iri_rs::IriBuf;
    use sea_query::{PostgresQueryBuilder, Query};

    fn compiled(projection: &Projection) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().projected_members(ProjectedMembers::of(projection)).build())
            .window(
                TemporalWindow::builder()
                    .timeproperty(TimeProp::Observed)
                    .interval(TimeInterval::After(
                        "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get(),
                    ))
                    .build(),
            )
            .instances(InstanceSelection::builder().build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::Normalized)
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn speed() -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: IriBuf::new("https://example.com/speed".to_string()).expect("valid IRI"),
            linked: Vec::new(),
        }
    }

    fn rendered(compiled: &CompiledTemporalQuery) -> Option<String> {
        projected_member_condition(&HistoryBranch::live(), compiled)
            .map(|condition| Query::select().cond_where(condition).to_owned().to_string(PostgresQueryBuilder))
    }

    /// A projection that keeps `id` leaves every entity a member.
    #[test]
    fn a_projection_keeping_id_adds_no_condition() {
        assert_eq!(rendered(&compiled(&Projection::builder().pick(vec![ProjectionTerm::Id]).build())), None);
        assert_eq!(rendered(&compiled(&Projection::default())), None);
    }

    /// Clause 5.7.4.5: a `pick` keeps an entity only through an in-window
    /// instance of a picked Attribute.
    #[test]
    fn a_pick_probes_the_represented_instances_of_the_picked_attributes() {
        let sql = rendered(&compiled(&Projection::builder().pick(vec![speed()]).build())).expect("a condition");
        assert!(sql.contains(r#""an"."name" IN ('https://example.com/speed')"#), "{sql}");
        assert!(sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
        assert!(sql.contains(r#""ah"."observed_at" >="#), "the window bounds the instances: {sql}");
        assert!(!sql.contains("scope_history"), "the pick does not keep the scope: {sql}");
    }

    /// Clause 5.7.4.4 step S8 precedes the projection, so the instances the
    /// `datasetId` parameter removes keep no entity alive.
    #[test]
    fn the_dataset_step_bounds_the_instances_that_keep_an_entity() {
        let mut query = compiled(&Projection::builder().pick(vec![speed()]).build());
        query.instances.dataset_id_filter = vec![DatasetSelector::parse("urn:ngsi-ld:Dataset:one").expect("a valid dataset selector")];
        let sql = rendered(&query).expect("a condition");
        assert!(sql.contains("dataset_id"), "{sql}");
    }

    /// An `omit` of `id` and `type` keeps an entity through any other
    /// Attribute's instance, or through the scope it spares.
    #[test]
    fn an_omit_probes_every_other_attribute_and_the_spared_scope() {
        let sql = rendered(&compiled(
            &Projection::builder().omit(vec![ProjectionTerm::Id, ProjectionTerm::Type, speed()]).build(),
        ))
        .expect("a condition");
        assert!(sql.contains(r#""an"."name" NOT IN ('https://example.com/speed'"#), "{sql}");
        assert!(sql.contains(r#"FROM "scope_history""#), "the spared scope keeps an entity: {sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }
}
