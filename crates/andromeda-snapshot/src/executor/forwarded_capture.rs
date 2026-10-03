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
use andromeda_ngsi_ld_entity::{admission::attribute_nesting_admission::admit_document_nesting, entity_state::EntityState, refusal::EntityRefusal};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, error::ExpansionError, normalization::Normalization};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::{
    attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit},
    sys_attrs::SysAttrsInclusion,
};
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
    /// A merged entity nests an attribute deeper than the branch could read
    /// back once it holds it.
    #[error("failed to capture entity: {0}")]
    AttributeNesting(#[from] AttributeNestingExceeded),
}

/// The merge of one capture's local states with its source entities, under
/// the query's context.
pub(crate) struct ForwardedCapture<'e> {
    /// The engine the context is processed with.
    engine: &'e JsonLdEngine,
    /// The context the snapshot query is evaluated under.
    context: &'e ContextSpec,
    /// The deepest nesting a captured attribute may have: a source entity
    /// enters the branch the way a write enters the live context.
    attribute_nesting: AttributeNestingLimit,
}

impl<'e> ForwardedCapture<'e> {
    /// The merge of captures evaluated under `context`, holding each captured
    /// attribute to `attribute_nesting`.
    pub(crate) const fn new(engine: &'e JsonLdEngine, context: &'e ContextSpec, attribute_nesting: AttributeNestingLimit) -> ForwardedCapture<'e> {
        ForwardedCapture {
            engine,
            context,
            attribute_nesting,
        }
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
    /// state, or nests an attribute deeper than the limit admits.
    pub(crate) async fn states(&self, entities: Vec<Entity>) -> Result<Vec<EntityState>, MergedCaptureError> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let expansion = EntityExpansion::prepare(self.engine, self.context).await?;
        let mut states = Vec::with_capacity(entities.len());
        for entity in &entities {
            let document = expansion.entity(entity, Normalization::Entity).await?;
            admit_document_nesting(self.attribute_nesting, &document)?;
            states.push(EntityState::try_from(document)?);
        }
        Ok(states)
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::forwarded_capture::{ForwardedCapture, MergedCaptureError};
    use andromeda_federation::outcome::ForwardedEntities;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::{entity::Entity, registration::forwarding_mode::ForwardingMode};
    use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
    use chrono::Utc;
    use serde_json::json;
    use std::num::NonZeroUsize;

    /// The limit every capture of these tests holds attributes to.
    fn limit() -> AttributeNestingLimit {
        AttributeNestingLimit::new(NonZeroUsize::new(4).expect("a positive limit"))
    }

    /// A source entity whose `reading` value nests `levels` objects.
    fn nested_source_entity(levels: usize) -> Entity {
        let value = (0..levels).fold(json!("leaf"), |inner, _| json!({"k": inner}));
        serde_json::from_value(json!({"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "reading": {"type": "Property", "value": value}}))
            .expect("a valid entity")
    }

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
        let states = ForwardedCapture::new(&engine, &context, limit())
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
        let merged = ForwardedCapture::new(&engine, &context, limit())
            .merged(&[local_state()], claimed, Utc::now())
            .await
            .expect("merges");
        pretty_assertions::assert_eq!(merged.len(), 1);
        assert!(merged[0].attributes().contains(SPEED), "{:?}", merged[0]);
        assert!(merged[0].attributes().contains(BRAND), "{:?}", merged[0]);
    }

    /// A source entity enters the branch the way a write enters the live
    /// context: an attribute nesting past the limit would be stored where no
    /// read could decode it, so the capture refuses it naming the attribute.
    #[tokio::test]
    async fn a_source_entity_nesting_past_the_limit_is_refused() {
        let engine = engine();
        let context = ContextSpec::core_only();
        let capture = ForwardedCapture::new(&engine, &context, limit());
        let captured = capture.states(vec![nested_source_entity(4)]).await.expect("four levels are within the limit");
        pretty_assertions::assert_eq!(captured.len(), 1);
        let refused = capture.states(vec![nested_source_entity(5)]).await.expect_err("five levels pass the limit");
        let MergedCaptureError::AttributeNesting(exceeded) = refused else {
            panic!("refused for its nesting: {refused}");
        };
        pretty_assertions::assert_eq!(exceeded.attribute.as_str(), "https://uri.etsi.org/ngsi-ld/default-context/reading");
        pretty_assertions::assert_eq!(exceeded.limit, limit());
    }

    /// Nothing to merge prepares no expansion and captures nothing.
    #[tokio::test]
    async fn no_source_entity_captures_nothing() {
        let engine = engine();
        let context = ContextSpec::core_only();
        let states = ForwardedCapture::new(&engine, &context, limit())
            .states(Vec::new())
            .await
            .expect("nothing to expand");
        pretty_assertions::assert_eq!(states.len(), 0);
    }
}
