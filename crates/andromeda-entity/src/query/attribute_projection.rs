//! Filtering a compact entity's attributes down to what the client asked
//! for (clause 4.21 `pick` and `omit`, clause 5.7.2 `attrs`).
//!
//! An entity read from the local store is narrowed before it is compacted
//! (`crate::query::state_projection`); these narrow an entity that arrives
//! compact -- a Linked Entity another broker returned, or an entity on its way
//! to a subscriber -- whose member names have to be expanded to be matched.

use crate::{error::EntityError, query::attribute_expansion::AttributeExpansions};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
use iri_rs::IriBuf;

/// Apply one compiled `pick`/`omit` projection to a single entity.
///
/// The attribute members the projection excludes are removed, and the
/// clause 4.21 structural rule is applied to `scope`. `id` and `type`
/// are members of the *rendered* representation, not of the typed
/// [`Entity`], so their half of the rule belongs to the renderer.
///
/// Public because a subscription's `notification.pick`/`omit` (clause
/// 5.8.6) is the same clause 4.21 projection applied to an entity on its
/// way to a subscriber rather than to a client.
///
/// # Errors
///
/// Returns [`EntityError`] when an attribute member name cannot be expanded
/// through the context.
pub async fn project_entity(entity: &mut Entity, projection: &Projection, expansions: &mut AttributeExpansions<'_>) -> Result<(), EntityError> {
    if !projection.pick.is_empty() || !projection.omit.is_empty() {
        expansions.cover(entity).await?;
    }

    if !projection.pick.is_empty() {
        entity.retain_attribute_members(|name| {
            expansions
                .expanded(name)
                .is_some_and(|iri| projection.pick.iter().any(|term| term.matches_attribute(name, iri)))
        });
    }

    if !projection.omit.is_empty() {
        // Clause 4.21: `omit=rel{a}` omits `a` inside the entity `rel` links
        // to, not `rel` itself. Only a bare `omit` term -- one carrying no
        // linked sub-projection -- removes the attribute from the entity being
        // projected here.
        entity.retain_attribute_members(|name| {
            !expansions.expanded(name).is_some_and(|iri| {
                projection
                    .omit
                    .iter()
                    .filter(|term| term.matches_attribute(name, iri))
                    .any(|term| term.linked_terms().is_empty())
            })
        });
    }

    apply_structural_scope_projection(entity, projection);
    Ok(())
}

/// Retain only the attributes whose expanded IRI appears in `wanted_iris`.
///
/// Public for the same reason as [`project_entity`]: a subscription's
/// `notification.attributes` (clause 5.8.6) is this narrowing applied on the
/// way to a subscriber.
///
/// # Errors
///
/// Returns [`EntityError`] when an attribute member name cannot be expanded
/// through the context.
pub async fn filter_entity_attrs(entity: &mut Entity, wanted_iris: &[IriBuf], expansions: &mut AttributeExpansions<'_>) -> Result<(), EntityError> {
    expansions.cover(entity).await?;
    entity.retain_attribute_members(|name| expansions.expanded(name).is_some_and(|iri| wanted_iris.contains(iri)));
    Ok(())
}

/// Apply the clause 4.21 structural-member rule to `scope`: with a
/// non-empty `pick`, `scope` survives only when [`ProjectionTerm::Scope`] is
/// explicitly listed; a non-empty `omit` drops `scope` when it lists
/// [`ProjectionTerm::Scope`]. `id` and `type` are members of the *rendered*
/// representation rather than of the typed [`Entity`], so their half of this
/// rule is applied by the renderer instead
/// (`EntityRenderer::with_structural_projection`).
pub(crate) fn apply_structural_scope_projection(entity: &mut Entity, projection: &Projection) {
    if !projection.keeps(&ProjectionTerm::Scope) {
        entity.scope = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::query::attribute_projection::apply_structural_scope_projection;
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_query::projection::{compiled::Projection, term::ProjectionTerm};
    use iri_rs::IriBuf;
    use serde_json::json;

    fn entity_with_scope() -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "scope": "/Madrid"
        }))
        .expect("entity")
    }

    /// Clause 4.21: `omit` removes listed structural members, and `scope`
    /// is one of the three keywords `pick`/`omit` may name.
    #[test]
    fn omit_of_scope_removes_scope_from_the_typed_entity() {
        let mut entity = entity_with_scope();
        let projection = Projection {
            pick: Vec::new(),
            omit: vec![ProjectionTerm::Scope],
        };

        apply_structural_scope_projection(&mut entity, &projection);

        assert!(entity.scope.is_none());
    }

    /// Clause 4.21, literal reading: with a non-empty `pick`, a structural
    /// member survives only when it is explicitly listed by keyword, so a
    /// `pick` that names only an attribute drops `scope`.
    #[test]
    fn a_non_empty_pick_without_the_scope_keyword_drops_scope() {
        let mut entity = entity_with_scope();
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/speed"),
                linked: Vec::new(),
            }],
            omit: Vec::new(),
        };

        apply_structural_scope_projection(&mut entity, &projection);

        assert!(entity.scope.is_none());
    }

    /// Clause 4.21: `scope` survives a non-empty `pick` when the pick list
    /// explicitly names the `scope` keyword.
    #[test]
    fn a_pick_naming_scope_keeps_scope() {
        let mut entity = entity_with_scope();
        let projection = Projection {
            pick: vec![ProjectionTerm::Scope],
            omit: Vec::new(),
        };

        apply_structural_scope_projection(&mut entity, &projection);

        assert!(entity.scope.is_some());
    }

    /// An empty `pick` and empty `omit` select every member with no
    /// restriction, so `scope` is left untouched.
    #[test]
    fn an_empty_projection_leaves_scope_untouched() {
        let mut entity = entity_with_scope();

        apply_structural_scope_projection(&mut entity, &Projection::default());

        assert!(entity.scope.is_some());
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }
}
