//! Filtering an entity's attributes down to what the client asked for
//! (clause 6.3.15 `pick` and `omit`, clause 5.7.2 `attrs`).

use crate::{error::AndromedaError, service::entity::query::EntityQueryService};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use iri_rs::IriBuf;

impl EntityQueryService {
    /// Apply `attrs`, `pick` and `omit` to every entity in a result set.
    pub(super) async fn apply_projection(
        &self,
        entities: Vec<Entity>,
        query: &CompiledEntityQuery,
        context: &ContextSpec,
    ) -> Result<Vec<Entity>, AndromedaError> {
        if query.projection.pick.is_empty() && query.projection.omit.is_empty() && query.selection.attrs.is_empty() {
            return Ok(entities);
        }

        let mut projected = Vec::with_capacity(entities.len());
        for mut entity in entities {
            if !query.selection.attrs.is_empty() {
                self.filter_entity_attrs(&mut entity, &query.selection.attrs, context).await?;
            }

            if !query.projection.pick.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.expanded_key(key.as_str(), context).await?;
                    if !query.projection.pick.iter().any(|term| term.matches_attribute(key.as_str(), &iri)) {
                        keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                    }
                }
                for key in keys_to_remove {
                    entity.attrs.shift_remove(&key);
                }
            }

            if !query.projection.omit.is_empty() {
                let mut keys_to_remove = Vec::new();
                for key in entity.attrs.keys() {
                    let iri = self.expanded_key(key.as_str(), context).await?;
                    if query.projection.omit.iter().any(|term| term.matches_attribute(key.as_str(), &iri)) {
                        keys_to_remove.push(key.clone()); // clone: owned key to avoid borrowing entity.attrs during removal
                    }
                }
                for key in keys_to_remove {
                    entity.attrs.shift_remove(&key);
                }
            }
            projected.push(entity);
        }

        Ok(projected)
    }

    /// Retain only the attributes whose expanded IRI appears in `wanted_iris`.
    pub(super) async fn filter_entity_attrs(&self, entity: &mut Entity, wanted_iris: &[IriBuf], context: &ContextSpec) -> Result<(), AndromedaError> {
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
