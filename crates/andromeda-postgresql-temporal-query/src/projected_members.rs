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
    alias::{An, Te},
    history::{base_history_source, represented_instances},
    scope_history::scope_is_selected,
};
use andromeda_ngsi_ld_query::{
    projection::projected_members::{ProjectedMembers, ScopeRetention},
    temporal::compiled::CompiledTemporalQuery,
};
use andromeda_postgresql_query::scope_validity::{ScopeEvent, overlaps_window, positioned_on_axis, scope_history_axis};
use andromeda_postgresql_schema::{
    current::E,
    name_dictionary::AttributeNames,
    temporal::{ScopeHistory, TemporalEntities},
};
use andromeda_postgresql_seaquery::{branch::qualified, literal::int_literal};
use sea_query::{Condition, DynIden, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The condition an entity of the stage 1 `e` relation has to satisfy for the
/// query's projection to leave it a member; `None` when every entity does.
#[must_use]
pub fn projected_member_condition(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> Option<Condition> {
    let projected = &compiled.entities.projected_members;
    let attribute_name = Expr::col((An, AttributeNames::Name));
    let (kept_names, scope) = match projected {
        ProjectedMembers::Unconditional => return None,
        ProjectedMembers::AnyOf { scope, .. } => (attribute_name.is_in(projected.names()), *scope),
        ProjectedMembers::AnyBesides { scope, .. } => (attribute_name.is_not_in(projected.names()), *scope),
    };
    let attribute = Expr::exists(represented_attribute(schema, compiled, kept_names));
    Some(match scope {
        ScopeRetention::Kept if scope_is_selected(compiled) => Condition::any().add(attribute).add(Expr::exists(represented_scope(schema, compiled))),
        ScopeRetention::Kept | ScopeRetention::Removed => Condition::all().add(attribute),
    })
}

/// The instances of the entity stage 2 would represent, restricted to the
/// Attributes whose names satisfy `kept_names`.
fn represented_attribute(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery, kept_names: SimpleExpr) -> SelectStatement {
    let mut probe = Query::select();
    probe.expr(int_literal(1));
    base_history_source(&mut probe, schema);
    probe
        .cond_where(Expr::col((Te, TemporalEntities::TemporalEntityKey)).equals((E, TemporalEntities::TemporalEntityKey)))
        .cond_where(represented_instances(schema, compiled))
        .cond_where(kept_names);
    probe.take()
}

/// The scope events of the entity whose clause 4.18 validity overlaps the
/// window, which is what the scope half of stage 2 represents.
fn represented_scope(schema: Option<&DynIden>, compiled: &CompiledTemporalQuery) -> SelectStatement {
    let axis = scope_history_axis(compiled.window.timeproperty);
    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from(qualified(schema, ScopeHistory::Table).alias(ScopeEvent))
        .cond_where(Expr::col((ScopeEvent, ScopeHistory::TemporalEntityKey)).equals((E, TemporalEntities::TemporalEntityKey)))
        .cond_where(positioned_on_axis(axis))
        .cond_where(overlaps_window(axis, compiled.window.interval, schema));
    probe.take()
}

#[cfg(test)]
mod tests {
    use crate::projected_members::projected_member_condition;
    use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
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
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use sea_query::{PostgresQueryBuilder, Query};

    fn compiled(projection: &Projection) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().projected_members(ProjectedMembers::of(projection)).build())
            .window(
                TemporalWindow::builder()
                    .timeproperty(TimeProp::Observed)
                    .interval(TimeInterval::After(
                        DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z").expect("valid instant").with_timezone(&Utc),
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
        projected_member_condition(None, compiled).map(|condition| Query::select().cond_where(condition).to_owned().to_string(PostgresQueryBuilder))
    }

    /// A projection that keeps `id` leaves every entity a member.
    #[test]
    fn a_projection_keeping_id_adds_no_condition() {
        assert_eq!(rendered(&compiled(&Projection::builder().pick(vec![ProjectionTerm::Id]).build())), None);
        assert_eq!(rendered(&compiled(&Projection::default())), None);
    }

    /// Clause 5.7.4.5 (ETSI TP `019_17_01` for the non-temporal twin): a `pick`
    /// keeps an entity only through an in-window instance of a picked Attribute.
    #[test]
    fn a_pick_probes_the_represented_instances_of_the_picked_attributes() {
        let sql = rendered(&compiled(&Projection::builder().pick(vec![speed()]).build())).expect("a condition");
        assert!(sql.contains(r#""an"."name" IN ('https://example.com/speed')"#), "{sql}");
        assert!(sql.contains(r#""te"."temporal_entity_key" = "e"."temporal_entity_key""#), "{sql}");
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
