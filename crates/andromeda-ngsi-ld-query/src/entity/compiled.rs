//! The compiled, context-expanded form of an entity query.

use crate::{
    candidate_filters::CandidateFilters,
    entity_selection::EntitySelection,
    ordering::result_ordering::ResultOrdering,
    projection::expanded_projection::ExpandedProjection,
};
use typed_builder::TypedBuilder;

/// A fully compiled entity query, ready to be lowered to a store query.
///
/// Produced by expanding and validating `EntityQueryParameters` against the
/// active JSON-LD context. All identifiers are expanded IRIs.
///
/// The parts are held separately because they change for unrelated reasons:
/// [`EntitySelection`] decides which rows come back, [`ExpandedProjection`] and
/// [`ResultOrdering`] decide only what is done with them, and the SQL builders
/// read one or the other rather than the whole record.
///
/// The page the query answers with is no part of it: the request boundary
/// decides the window once, and a store is handed it beside the compiled
/// query, so one compilation serves every page of the same query.
#[derive(Debug, Clone, TypedBuilder)]
pub struct CompiledEntityQuery {
    /// Which entities match.
    pub selection: EntitySelection,
    /// Which members of a matching entity reach the response (clause 4.21).
    #[builder(default = Default::default())]
    pub projection: ExpandedProjection,
    /// How matching entities are ordered (clause 4.23).
    #[builder(default = Default::default())]
    pub ordering: ResultOrdering,
}

impl CompiledEntityQuery {
    /// The query a local read runs, held to the filters `filters` says.
    ///
    /// Clauses 5.7.2.4 and 5.14.4.4: under split entities the local query
    /// returns every Entity found locally whose identifier, id pattern and
    /// type match. The value, geo and scope filters and the Attribute list are
    /// applied to the aggregated Entities, and clause 5.7.2.5 then reduces
    /// each matching Entity by the projection, so a deferring read takes each
    /// local part whole.
    #[must_use]
    pub fn held_to(mut self, filters: CandidateFilters) -> CompiledEntityQuery {
        match filters {
            CandidateFilters::Applied => {}
            CandidateFilters::Deferred => {
                self.selection.without_split_filters();
                self.selection.without_attribute_requirement();
                self.projection = ExpandedProjection::default();
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate_filters::CandidateFilters,
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        projection::{
            expanded_projection::ExpandedProjection,
            expanded_projection_term::ExpandedProjectionTerm,
            projected_members::{ProjectedMembers, ScopeRetention},
        },
        scope::scope_query::ScopeQuery,
    };
    use iri_rs::IriBuf;

    fn speed() -> IriBuf {
        IriBuf::new("https://example.com/speed".to_owned()).expect("a valid IRI")
    }

    /// A query listing `speed`, picking it and holding the Entities to the
    /// Scope `/Madrid`.
    fn picking_speed_in_madrid() -> CompiledEntityQuery {
        let picked = ExpandedProjection::builder()
            .pick(vec![ExpandedProjectionTerm::Attribute {
                iri: speed(),
                linked: Vec::new(),
            }])
            .build();
        CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .attrs(vec![speed()])
                    .scope_q(Some("/Madrid".parse::<ScopeQuery>().expect("a valid scopeQ")))
                    .projected_members(ProjectedMembers::AnyOf {
                        attributes: vec![speed()],
                        scope: ScopeRetention::Removed,
                    })
                    .build(),
            )
            .projection(picked)
            .build()
    }

    /// Clause 5.7.2.4: a split query's local part keeps neither the filters
    /// nor the Attribute list, and clause 5.7.2.5 leaves the projection to
    /// the matching aggregated Entity.
    #[test]
    fn a_deferring_read_takes_each_local_part_whole() {
        let parts = picking_speed_in_madrid().held_to(CandidateFilters::Deferred);

        pretty_assertions::assert_eq!(parts.selection.attrs, Vec::<IriBuf>::new());
        assert!(parts.selection.scope_q.is_none());
        pretty_assertions::assert_eq!(parts.selection.projected_members, ProjectedMembers::Unconditional);
        assert!(parts.projection.is_empty());
    }

    /// Without split entities the local read meets every filter of the query
    /// and answers each Entity reduced by its projection.
    #[test]
    fn an_applying_read_keeps_every_filter_and_the_projection() {
        let held = picking_speed_in_madrid().held_to(CandidateFilters::Applied);

        pretty_assertions::assert_eq!(held.selection.attrs, vec![speed()]);
        assert!(held.selection.scope_q.is_some());
        assert!(held.selection.projected_members.constrains());
        assert!(!held.projection.is_empty());
    }
}
