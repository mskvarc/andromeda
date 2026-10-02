//! Turning the entities a capture merged with its Context Sources into states
//! (clause 5.16.1.4).
//!
//! A source answers in compact JSON-LD, and the clause 4.5.5 merge of a local
//! entity with what its sources answered for it works on the compact entity.
//! So a captured local state a source also answered for is compacted once
//! under the query's context, merged, and the merged entity -- like a source
//! entity no local page held -- is expanded once under the same context into
//! the state the Snapshot's branch holds. A local state no source answered for
//! never takes this path.

use andromeda_entity::query::EntityQueryService;
use andromeda_federation::outcome::ForwardedEntities;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::{
    compaction_error::CompactionError,
    entity_compactor::{EntityCompactor, EntityRead},
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, refusal::EntityRefusal};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, error::ExpansionError, normalization::Normalization};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use thiserror::Error;

/// Why a merged capture could not be turned into the states the branch holds.
#[derive(Debug, Error)]
pub(crate) enum MergedCaptureError {
    /// A local state a source also answered for did not compact under the
    /// query's context.
    #[error("failed to compact captured entity: {0}")]
    Compaction(#[from] CompactionError),
    /// A merged entity did not expand under the query's context.
    #[error("failed to expand captured entity: {0}")]
    Expansion(#[from] ExpansionError),
    /// A merged entity expanded into a document no entity state can be.
    #[error("failed to read captured entity: {0}")]
    Refused(#[from] EntityRefusal),
}

/// The merge of one capture's local states with its source entities, under
/// the query's context.
pub(crate) struct ForwardedCapture<'e> {
    /// The engine the context is processed with.
    engine: &'e JsonLdEngine,
    /// The context the snapshot query is evaluated under.
    context: &'e ContextSpec,
}

impl<'e> ForwardedCapture<'e> {
    /// The merge of captures evaluated under `context`.
    pub(crate) const fn new(engine: &'e JsonLdEngine, context: &'e ContextSpec) -> ForwardedCapture<'e> {
        ForwardedCapture { engine, context }
    }

    /// The local states, merged with the source entities `claimed` for them,
    /// as states. `captured_at` is the instant the local states were narrowed
    /// at.
    ///
    /// # Errors
    ///
    /// Returns [`MergedCaptureError`] when a state does not compact or a merged
    /// entity does not expand back.
    pub(crate) async fn merged(
        &self,
        local: &[EntityState],
        claimed: Vec<ForwardedEntities>,
        captured_at: DateTime<Utc>,
    ) -> Result<Vec<EntityState>, MergedCaptureError> {
        let compactor = self.engine.compactor(self.context);
        let entity_compactor = EntityCompactor::new(&compactor);
        // A captured state's instance system times are not part of the merge
        // (clause 4.8): the merged entity is a write the branch records anew.
        let read = EntityRead {
            id_override: None,
            sys_attrs: SysAttrsInclusion::Strip,
            now: captured_at,
        };
        let mut compact = Vec::with_capacity(local.len());
        for state in local {
            compact.push(entity_compactor.entity(state, &read).await?);
        }
        self.states(EntityQueryService::merge_with_forwarded(compact, claimed, None)).await
    }

    /// Merged source entities as states.
    ///
    /// # Errors
    ///
    /// Returns [`MergedCaptureError`] when an entity does not expand into a
    /// state.
    pub(crate) async fn states(&self, entities: Vec<Entity>) -> Result<Vec<EntityState>, MergedCaptureError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let expansion = EntityExpansion::prepare(self.engine, self.context).await?;
        let mut states = Vec::with_capacity(entities.len());
        for entity in &entities {
            let document = expansion.entity(entity, Normalization::Entity).await?;
            states.push(EntityState::try_from(document)?);
        }
        Ok(states)
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::forwarded_capture::ForwardedCapture;
    use andromeda_federation::outcome::ForwardedEntities;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::{entity::Entity, registration::forwarding_mode::ForwardingMode};
    use chrono::Utc;
    use serde_json::json;

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    /// An engine that resolves only what is embedded, never the network.
    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    fn source_entity(attribute: &str, value: i64) -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            attribute: {"type": "Property", "value": value}
        }))
        .expect("a valid entity")
    }

    fn local_state() -> EntityState {
        let stored = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            SPEED: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 10}]}]
        }]);
        EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state")
    }

    /// A source entity no local page held is expanded once under the query's
    /// context into the state the branch holds.
    #[tokio::test]
    async fn a_source_entity_is_captured_as_its_expanded_state() {
        let engine = engine();
        let context = ContextSpec::core_only();
        let states = ForwardedCapture::new(&engine, &context)
            .states(vec![source_entity("speed", 3)])
            .await
            .expect("expands");
        pretty_assertions::assert_eq!(states.len(), 1);
        pretty_assertions::assert_eq!(states[0].id().as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(states[0].attributes().contains(SPEED), "{:?}", states[0]);
    }

    /// A local state a source also answered for is merged with the source's
    /// attributes (clause 4.5.5) and captured as one state.
    #[tokio::test]
    async fn a_local_state_merges_the_attributes_its_source_answered() {
        let engine = engine();
        let context = ContextSpec::core_only();
        let claimed = vec![ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: vec![source_entity("brand", 4)],
            warnings: Vec::new(),
            linked_map: None,
        }];
        let merged = ForwardedCapture::new(&engine, &context)
            .merged(&[local_state()], claimed, Utc::now())
            .await
            .expect("merges");
        pretty_assertions::assert_eq!(merged.len(), 1);
        assert!(merged[0].attributes().contains(SPEED), "{:?}", merged[0]);
        assert!(merged[0].attributes().contains(BRAND), "{:?}", merged[0]);
    }

    /// Nothing to merge prepares no expansion and captures nothing.
    #[tokio::test]
    async fn no_source_entity_captures_nothing() {
        let engine = engine();
        let context = ContextSpec::core_only();
        let states = ForwardedCapture::new(&engine, &context).states(Vec::new()).await.expect("nothing to expand");
        pretty_assertions::assert_eq!(states.len(), 0);
    }
}
