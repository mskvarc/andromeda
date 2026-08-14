//! Preparing the clause 4.21 nested projections in the shape the requested
//! join mode applies them.
//!
//! The two join modes narrow linked entities by different rules, and clause
//! 4.5.23 is what forces the difference. `join=flat` returns each linked
//! entity exactly once, so a target reached by several Relationships can only
//! be rendered one way: the union rule of clause 4.21 settles it, and the
//! narrowing happens here, on the entity itself. `join=inline` nests a
//! separate copy of the target under every Relationship that names it, so each
//! copy follows the sub-projection its own Relationship carried, and the
//! narrowing happens while the renderer walks those Relationships.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, compiled::Projection};
use andromeda_ngsi_ld_request::join_traversal::RetrievingJoin;
use indexmap::IndexMap;

impl EntityQueryService {
    /// Apply the clause 4.21 nested projections to a resolved join.
    ///
    /// For [`RetrievingJoin::Flat`] the linked entities are narrowed in place
    /// by the union rule and the caller has nothing further to do, which is
    /// what the `None` return says. For [`RetrievingJoin::Inline`] the
    /// entities are left whole and the returned index is handed to the
    /// renderer, which applies the sub-projections one Relationship instance
    /// at a time.
    ///
    /// The parameter is a [`RetrievingJoin`] rather than a `JoinMode` because
    /// `join=@none` and an absent `join` retrieve no linked entity at all:
    /// there is nothing for a nested projection to apply to, and taking the
    /// narrower type means that case cannot reach here to be handled by an
    /// arm no caller can take.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub async fn prepare_join_projection(
        &self,
        roots: &[Entity],
        linked: &mut IndexMap<Uri, Entity>,
        projection: &Projection,
        context: &ContextSpec,
        join: RetrievingJoin,
    ) -> Result<Option<AttributeIriIndex>, EntityError> {
        // Only a term carrying a `{...}` sub-projection says anything about a
        // linked entity (clause 4.21). A `pick`/`omit` of plain attribute
        // names narrows the roots and nothing else, so neither the union pass
        // nor the expansion index is worth building.
        let nests = projection.pick.iter().chain(projection.omit.iter()).any(|term| !term.linked_terms().is_empty());
        if !nests {
            return Ok(None);
        }
        match join {
            RetrievingJoin::Flat => {
                self.apply_flat_linked_projections(roots, linked, projection, context).await?;
                Ok(None)
            }
            RetrievingJoin::Inline => {
                let index = self.attribute_iri_index(roots.iter().chain(linked.values()), context).await?;
                Ok(Some(index))
            }
        }
    }
}
