//! Merging the entities a capture read with what its Context Sources answered
//! for them, and turning an entity a source contributed to into the state the
//! Snapshot's branch holds (clause 5.16.1.4).
//!
//! Every source's answer was read into the model under the context it was
//! written in, so a captured local state a source also answered for merges
//! with what the source answered on the model (clause 4.5.5). The merged
//! entity, like a source entity no local page held, is a write the branch
//! records anew; a local state no source answered for never takes this path.

use crate::executor::captured_page::CandidateEntity;
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::outcome::ForwardedEntities;
use andromeda_ngsi_ld_entity::{admission::attribute_nesting_admission::admit_document_nesting, document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// How one capture records the entities its Context Sources contributed to.
pub(crate) struct ForwardedCapture {
    /// The deepest nesting a captured attribute may have: a source entity
    /// enters the branch the way a write enters the live context.
    attribute_nesting: AttributeNestingLimit,
}

impl ForwardedCapture {
    /// The capture holding each attribute a source contributed to
    /// `attribute_nesting`.
    pub(crate) const fn new(attribute_nesting: AttributeNestingLimit) -> ForwardedCapture {
        ForwardedCapture { attribute_nesting }
    }

    /// The entities of one local page: each state a source answered for,
    /// merged with the source entities `claimed` for it, and every other
    /// state as stored.
    pub(crate) fn local_candidates(local: Vec<EntityState>, claimed: Vec<ForwardedEntities>) -> Vec<CandidateEntity> {
        if claimed.is_empty() {
            return local.into_iter().map(CandidateEntity::stored).collect();
        }
        let (answered, unanswered): (Vec<EntityState>, Vec<EntityState>) = {
            let answered_ids: HashSet<&Identifier> = claimed
                .iter()
                .flat_map(|answer| answer.entities.iter().map(|received| received.state.id()))
                .collect();
            local.into_iter().partition(|state| answered_ids.contains(state.id()))
        };
        let versions = answered
            .into_iter()
            .map(|state| ReceivedState {
                state,
                system_times: EntitySystemTimes::default(),
                linked: Vec::new(),
            })
            .collect();
        EntityQueryService::merge_with_forwarded(versions, claimed)
            .into_iter()
            .map(|merged| CandidateEntity::received(merged.state))
            .chain(unanswered.into_iter().map(CandidateEntity::stored))
            .collect()
    }

    /// `state`, which a source contributed to, as the write the branch records
    /// at `captured_at`.
    ///
    /// Every instance, at every depth, takes `captured_at` as its clause 4.8
    /// `createdAt` and `modifiedAt` in place of whatever times it was read with
    /// or without: a source answers without them unless asked for
    /// `sysAttrs`, and the branch stores no instance that lacks them.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNestingExceeded`] when `state` nests an attribute
    /// deeper than the branch could read back once it holds it.
    pub(crate) fn written(&self, mut state: EntityState, captured_at: DateTime<Utc>) -> Result<EntityState, AttributeNestingExceeded> {
        admit_document_nesting(self.attribute_nesting, &EntityDocument::from(&state))?;
        state.stamp_attribute_instants(None, SystemInstant::new(captured_at));
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::{captured_page::StateOrigin, forwarded_capture::ForwardedCapture};
    use andromeda_federation::outcome::ForwardedEntities;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, registration::forwarding_mode::ForwardingMode};
    use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::{iter::once, num::NonZeroUsize};

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";
    const READING: &str = "https://uri.etsi.org/ngsi-ld/default-context/reading";
    const ACCURACY: &str = "https://uri.etsi.org/ngsi-ld/default-context/accuracy";

    /// The limit every capture of these tests holds attributes to.
    fn limit() -> AttributeNestingLimit {
        AttributeNestingLimit::new(NonZeroUsize::new(4).expect("a positive limit"))
    }

    fn state(id: &str, attribute: &str, value: &Value) -> EntityState {
        let stored = json!([{
            "@id": id,
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            attribute: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}]
        }]);
        EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state")
    }

    fn version(state: EntityState) -> ReceivedState {
        ReceivedState {
            state,
            system_times: EntitySystemTimes::default(),
            linked: Vec::new(),
        }
    }

    /// What a source answered for the entity `answered` holds.
    fn claimed(answered: EntityState) -> Vec<ForwardedEntities> {
        vec![ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: vec![version(answered)],
            warnings: Vec::new(),
            linked_map: None,
        }]
    }

    /// A source entity whose `reading` value nests `levels` objects.
    fn nested_source_entity(levels: usize) -> EntityState {
        let mut value = json!({"@value": "leaf"});
        for _ in 0..levels {
            value = json!({"https://uri.etsi.org/ngsi-ld/default-context/k": [value]});
        }
        let stored = json!([{
            "@id": "urn:ngsi-ld:Vehicle:2",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            READING: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [value]}]
        }]);
        EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state")
    }

    /// A local state a source also answered for is merged with the source's
    /// attributes (clause 4.5.5) into one entity a source contributed to.
    #[test]
    fn a_local_state_merges_the_attributes_its_source_answered() {
        let candidates = ForwardedCapture::local_candidates(
            vec![state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10))],
            claimed(state("urn:ngsi-ld:Vehicle:1", BRAND, &json!(4))),
        );
        pretty_assertions::assert_eq!(candidates.len(), 1);
        pretty_assertions::assert_eq!(candidates[0].origin, StateOrigin::Received);
        assert!(candidates[0].state.attributes().contains(SPEED), "{:?}", candidates[0]);
        assert!(candidates[0].state.attributes().contains(BRAND), "{:?}", candidates[0]);
    }

    /// A local state no source answered for stays as stored beside the one a
    /// source merged into.
    #[test]
    fn a_local_state_no_source_answered_for_stays_stored() {
        let candidates = ForwardedCapture::local_candidates(
            vec![
                state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10)),
                state("urn:ngsi-ld:Vehicle:3", SPEED, &json!(30)),
            ],
            claimed(state("urn:ngsi-ld:Vehicle:1", BRAND, &json!(4))),
        );
        let origins: Vec<(&str, StateOrigin)> = candidates.iter().map(|candidate| (candidate.state.id().as_str(), candidate.origin)).collect();
        pretty_assertions::assert_eq!(
            origins,
            vec![("urn:ngsi-ld:Vehicle:1", StateOrigin::Received), ("urn:ngsi-ld:Vehicle:3", StateOrigin::Stored)]
        );
    }

    /// A page no source answered for is every state as stored.
    #[test]
    fn a_page_no_source_answered_for_is_stored_whole() {
        let candidates = ForwardedCapture::local_candidates(vec![state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10))], Vec::new());
        pretty_assertions::assert_eq!(candidates.len(), 1);
        pretty_assertions::assert_eq!(candidates[0].origin, StateOrigin::Stored);
    }

    /// A source entity enters the branch the way a write enters the live
    /// context: an attribute nesting past the limit would be stored where no
    /// read could decode it, so the capture refuses it naming the attribute.
    #[test]
    fn a_source_entity_nesting_past_the_limit_is_refused() {
        let capture = ForwardedCapture::new(limit());
        let written = capture.written(nested_source_entity(4), Utc::now()).expect("four levels are within the limit");
        assert!(written.attributes().contains(READING));
        let refused = capture.written(nested_source_entity(5), Utc::now()).expect_err("five levels pass the limit");
        pretty_assertions::assert_eq!(refused.attribute.as_str(), READING);
        pretty_assertions::assert_eq!(refused.limit, limit());
    }

    /// The clause 4.8 times of the `attribute` instance, and of its `accuracy`
    /// sub-attribute when it has one.
    fn stamps(state: &EntityState, attribute: &str) -> Vec<(Option<SystemInstant>, Option<SystemInstant>)> {
        let instance = state.attributes().get(attribute).and_then(|held| held.get(None)).expect("the instance");
        let nested = instance.sub_attributes().get(ACCURACY).and_then(|held| held.get(None));
        once(instance)
            .chain(nested)
            .map(|instance| (instance.created_at(), instance.modified_at()))
            .collect()
    }

    /// A source answers without system times unless asked for `sysAttrs`;
    /// the branch stores none without them, so its entity, sub-attributes
    /// included, is written at the capture instant.
    #[test]
    fn a_source_entity_answered_without_system_times_is_written_at_the_capture_instant() {
        let stored = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            SPEED: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 5}],
                ACCURACY: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]
            }]
        }]);
        let source = EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state");
        pretty_assertions::assert_eq!(stamps(&source, SPEED), vec![(None, None), (None, None)]);
        let captured_at = Utc::now();
        let written = ForwardedCapture::new(limit()).written(source, captured_at).expect("captures");
        let at = Some(SystemInstant::new(captured_at));
        pretty_assertions::assert_eq!(stamps(&written, SPEED), vec![(at, at), (at, at)]);
    }

    /// A local state merged with a source's answer is a write the branch
    /// records anew: the times it was stored with give way to the capture
    /// instant, like those of the attribute the source added.
    #[test]
    fn a_merged_local_state_takes_the_capture_instant_in_place_of_its_stored_times() {
        let mut local = state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10));
        local.stamp_attribute_instants(None, "2020-01-01T00:00:00Z".parse().expect("RFC 3339"));
        let mut candidates = ForwardedCapture::local_candidates(vec![local], claimed(state("urn:ngsi-ld:Vehicle:1", BRAND, &json!(4))));
        let captured_at = Utc::now();
        let merged = candidates.pop().expect("the merged entity");
        let written = ForwardedCapture::new(limit()).written(merged.state, captured_at).expect("captures");
        let at = Some(SystemInstant::new(captured_at));
        pretty_assertions::assert_eq!(stamps(&written, SPEED), vec![(at, at)]);
        pretty_assertions::assert_eq!(stamps(&written, BRAND), vec![(at, at)]);
    }
}
