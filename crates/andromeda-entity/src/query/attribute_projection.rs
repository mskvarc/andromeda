//! Filtering an entity's attributes down to what the client asked for
//! (clause 4.21 `pick` and `omit`, clause 5.7.2 `attrs`).

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    projection::{compiled::Projection, term::ProjectionTerm},
};
use iri_rs::IriBuf;

impl EntityQueryService {
    /// Apply `attrs`, `pick` and `omit` to every entity in a result set.
    pub(super) async fn apply_projection(&self, entities: Vec<Entity>, query: &CompiledEntityQuery, context: &ContextSpec) -> Result<Vec<Entity>, EntityError> {
        if query.projection.pick.is_empty() && query.projection.omit.is_empty() && query.selection.attrs.is_empty() {
            return Ok(entities);
        }

        let mut projected = Vec::with_capacity(entities.len());
        for mut entity in entities {
            if !query.selection.attrs.is_empty() {
                self.filter_entity_attrs(&mut entity, &query.selection.attrs, context).await?;
            }

            self.project_entity(&mut entity, &query.projection, context).await?;
            // A `pick` reduces an entity to only its listed members (clause
            // 6.5.7). When it names only attributes the entity lacks and no
            // structural keyword, nothing survives; consistent with the `attrs`
            // rule that an entity having none of the requested attributes is not
            // returned (a 404 on single retrieval), such an entity is dropped
            // from a query result rather than emitted as an empty object.
            if !projects_to_empty(&entity, &query.projection) {
                projected.push(entity);
            }
        }

        Ok(projected)
    }

    /// Apply one compiled `pick`/`omit` projection to a single entity.
    ///
    /// The attribute members the projection excludes are removed, and the
    /// clause 4.21 structural rule is applied to `scope`. `id` and `type`
    /// are members of the *rendered* representation, not of the typed
    /// [`Entity`], so their half of the rule belongs to the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`EntityError`] when an attribute key cannot be expanded
    /// through the context.
    pub(crate) async fn project_entity(&self, entity: &mut Entity, projection: &Projection, context: &ContextSpec) -> Result<(), EntityError> {
        if !projection.pick.is_empty() {
            let mut keys_to_remove = Vec::new();
            for key in entity.attrs.keys() {
                let iri = self.expanded_key(key.as_str(), context).await?;
                if !projection.pick.iter().any(|term| term.matches_attribute(key.as_str(), &iri)) {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            for key in keys_to_remove {
                entity.attrs.shift_remove(&key);
            }
        }

        if !projection.omit.is_empty() {
            let mut keys_to_remove = Vec::new();
            for key in entity.attrs.keys() {
                let iri = self.expanded_key(key.as_str(), context).await?;
                // Clause 4.21: `omit=rel{a}` omits `a` inside the entity `rel`
                // links to, not `rel` itself. Only a bare `omit` term -- one
                // carrying no linked sub-projection -- removes the attribute
                // from the entity being projected here.
                let removed = projection
                    .omit
                    .iter()
                    .filter(|term| term.matches_attribute(key.as_str(), &iri))
                    .any(|term| term.linked_terms().is_empty());
                if removed {
                    keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                }
            }
            for key in keys_to_remove {
                entity.attrs.shift_remove(&key);
            }
        }

        apply_structural_scope_projection(entity, projection);
        Ok(())
    }

    /// Retain only the attributes whose expanded IRI appears in `wanted_iris`.
    pub(super) async fn filter_entity_attrs(&self, entity: &mut Entity, wanted_iris: &[IriBuf], context: &ContextSpec) -> Result<(), EntityError> {
        let mut keys_to_remove = Vec::new();
        for key in entity.attrs.keys() {
            let iri = self.expanded_key(key.as_str(), context).await?;
            if !wanted_iris.contains(&iri) {
                keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
            }
        }
        for key in keys_to_remove {
            entity.attrs.shift_remove(&key);
        }
        Ok(())
    }
}

/// Whether a `pick` projection has emptied an entity so completely that it
/// renders as `{}` and must be dropped from a query result set.
///
/// Only `pick` can strip an entity's structural members (clause 4.21), so
/// only `pick` can empty it: when it names none of `id`, `type` or `scope` and
/// the entity has no surviving attribute, nothing would render. An `omit` or
/// `attrs` selection always leaves `id` and `type`, so it never empties.
fn projects_to_empty(entity: &Entity, projection: &Projection) -> bool {
    !projection.pick.is_empty()
        && entity.attrs.is_empty()
        && !projection.pick.contains(&ProjectionTerm::Id)
        && !projection.pick.contains(&ProjectionTerm::Type)
        && !projection.pick.contains(&ProjectionTerm::Scope)
}

/// Apply the clause 4.21 structural-member rule to `scope`: with a
/// non-empty `pick`, `scope` survives only when [`ProjectionTerm::Scope`] is
/// explicitly listed; a non-empty `omit` drops `scope` when it lists
/// [`ProjectionTerm::Scope`]. `id` and `type` are members of the *rendered*
/// representation rather than of the typed [`Entity`], so their half of this
/// rule is applied by the renderer instead
/// (`EntityRenderer::with_structural_projection`).
pub(crate) fn apply_structural_scope_projection(entity: &mut Entity, projection: &Projection) {
    if !projection.pick.is_empty() && !projection.pick.contains(&ProjectionTerm::Scope) {
        entity.scope = None;
    }
    if !projection.omit.is_empty() && projection.omit.contains(&ProjectionTerm::Scope) {
        entity.scope = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn entity_with_name() -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"}
        }))
        .expect("entity")
    }

    /// Clause 5.7.2: a `pick` naming only an attribute the entity does not
    /// carry, and no structural keyword, empties the rendered entity, so it
    /// drops out of the result set.
    #[test]
    fn a_pick_matching_no_attribute_or_structural_member_empties_the_entity() {
        let entity = entity_with_scope();
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute { iri: iri("https://example.com/name"), linked: Vec::new() }],
            omit: Vec::new(),
        };

        assert!(projects_to_empty(&entity, &projection));
    }

    /// A `pick` naming a structural keyword keeps the entity: `id` still renders.
    #[test]
    fn a_pick_naming_a_structural_member_keeps_the_entity() {
        let entity = entity_with_scope();
        let projection = Projection { pick: vec![ProjectionTerm::Id], omit: Vec::new() };

        assert!(!projects_to_empty(&entity, &projection));
    }

    /// A surviving attribute keeps the entity in the result set.
    #[test]
    fn a_pick_leaving_a_surviving_attribute_keeps_the_entity() {
        let entity = entity_with_name();
        let projection = Projection {
            pick: vec![ProjectionTerm::Attribute { iri: iri("https://example.com/name"), linked: Vec::new() }],
            omit: Vec::new(),
        };

        assert!(!projects_to_empty(&entity, &projection));
    }

    /// `omit` always leaves `id` and `type`, so it never empties an entity.
    #[test]
    fn an_omit_never_empties_the_entity() {
        let entity = entity_with_scope();
        let projection = Projection { pick: Vec::new(), omit: vec![ProjectionTerm::Scope] };

        assert!(!projects_to_empty(&entity, &projection));
    }
}
