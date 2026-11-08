//! Turning the entities a capture merged with its Context Sources into states
//! (clause 5.16.1.4).
//!
//! Every source's answer was read into the model under the context it was
//! written in, so a captured local state a source also answered for merges
//! with what the source answered on the model (clause 4.5.5), and the merged
//! version, like a source entity no local page held, is the state the
//! Snapshot's branch holds. A local state no source answered for never takes
//! this path.

use andromeda_entity::query::EntityQueryService;
use andromeda_federation::outcome::ForwardedEntities;
use andromeda_ngsi_ld_entity::{admission::attribute_nesting_admission::admit_document_nesting, document::EntityDocument, entity_state::EntityState};
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
use andromeda_ngsi_ld_model::entity_system_times::EntitySystemTimes;
use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};
use chrono::{DateTime, Utc};

/// The merge of one capture's local states with its source entities.
pub(crate) struct ForwardedCapture {
    /// The deepest nesting a captured attribute may have: a source entity
    /// enters the branch the way a write enters the live context.
    attribute_nesting: AttributeNestingLimit,
}

impl ForwardedCapture {
    /// The merge of captures holding each captured attribute to
    /// `attribute_nesting`.
    pub(crate) const fn new(attribute_nesting: AttributeNestingLimit) -> ForwardedCapture {
        ForwardedCapture { attribute_nesting }
    }

    /// The local states, merged with the source entities `claimed` for them,
    /// as states. `captured_at` is the instant the local states were narrowed
    /// at.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNestingExceeded`] when a merged version nests an
    /// attribute deeper than the branch could read back once it holds it.
    pub(crate) fn merged(
        &self,
        local: Vec<EntityState>,
        claimed: Vec<ForwardedEntities>,
        captured_at: DateTime<Utc>,
    ) -> Result<Vec<EntityState>, AttributeNestingExceeded> {
        let versions = local
            .into_iter()
            .map(|state| ReceivedState {
                state,
                system_times: EntitySystemTimes::default(),
                linked: Vec::new(),
            })
            .collect();
        self.states(EntityQueryService::merge_with_forwarded(versions, claimed), captured_at)
    }

    /// Merged versions as the states the branch holds at `captured_at`.
    ///
    /// An instance expired at `captured_at` is not captured (clause 4.22). The
    /// captured entity is a write the branch records anew, so every instance,
    /// at every depth, takes `captured_at` as its clause 4.8 `createdAt` and
    /// `modifiedAt` in place of whatever times it was read with or without: a
    /// source answers without them unless asked for `sysAttrs`, and the
    /// branch stores no instance that lacks them.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNestingExceeded`] when a version nests an attribute
    /// deeper than the limit admits.
    pub(crate) fn states(&self, versions: Vec<ReceivedState>, captured_at: DateTime<Utc>) -> Result<Vec<EntityState>, AttributeNestingExceeded> {
        versions
            .into_iter()
            .map(|version| {
                let mut state = version.state;
                state.remove_expired_instances(captured_at);
                admit_document_nesting(self.attribute_nesting, &EntityDocument::from(&state))?;
                state.stamp_attribute_instants(None, SystemInstant::new(captured_at));
                Ok(state)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::forwarded_capture::ForwardedCapture;
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

    /// A source entity whose `reading` value nests `levels` objects.
    fn nested_source_entity(levels: usize) -> ReceivedState {
        let mut value = json!({"@value": "leaf"});
        for _ in 0..levels {
            value = json!({"https://uri.etsi.org/ngsi-ld/default-context/k": [value]});
        }
        let stored = json!([{
            "@id": "urn:ngsi-ld:Vehicle:2",
            "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
            READING: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [value]}]
        }]);
        version(EntityState::from_slice(stored.to_string().as_bytes()).expect("a stored state"))
    }

    /// A source entity no local page held is captured as the state it was
    /// read into.
    #[test]
    fn a_source_entity_is_captured_as_its_state() {
        let states = ForwardedCapture::new(limit())
            .states(vec![version(state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(3)))], Utc::now())
            .expect("captures");
        pretty_assertions::assert_eq!(states.len(), 1);
        pretty_assertions::assert_eq!(states[0].id().as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(states[0].attributes().contains(SPEED), "{:?}", states[0]);
    }

    /// A local state a source also answered for is merged with the source's
    /// attributes (clause 4.5.5) and captured as one state.
    #[test]
    fn a_local_state_merges_the_attributes_its_source_answered() {
        let claimed = vec![ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: vec![version(state("urn:ngsi-ld:Vehicle:1", BRAND, &json!(4)))],
            warnings: Vec::new(),
            linked_map: None,
        }];
        let merged = ForwardedCapture::new(limit())
            .merged(vec![state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10))], claimed, Utc::now())
            .expect("merges");
        pretty_assertions::assert_eq!(merged.len(), 1);
        assert!(merged[0].attributes().contains(SPEED), "{:?}", merged[0]);
        assert!(merged[0].attributes().contains(BRAND), "{:?}", merged[0]);
    }

    /// A source entity enters the branch the way a write enters the live
    /// context: an attribute nesting past the limit would be stored where no
    /// read could decode it, so the capture refuses it naming the attribute.
    #[test]
    fn a_source_entity_nesting_past_the_limit_is_refused() {
        let capture = ForwardedCapture::new(limit());
        let captured = capture
            .states(vec![nested_source_entity(4)], Utc::now())
            .expect("four levels are within the limit");
        pretty_assertions::assert_eq!(captured.len(), 1);
        let refused = capture
            .states(vec![nested_source_entity(5)], Utc::now())
            .expect_err("five levels pass the limit");
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
    /// included, is captured as written at the capture instant.
    #[test]
    fn a_source_entity_answered_without_system_times_is_stamped_at_the_capture_instant() {
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
        let states = ForwardedCapture::new(limit()).states(vec![version(source)], captured_at).expect("captures");
        let at = Some(SystemInstant::new(captured_at));
        pretty_assertions::assert_eq!(stamps(&states[0], SPEED), vec![(at, at), (at, at)]);
    }

    /// A local state merged with a source's answer is a write the branch
    /// records anew: the times it was stored with give way to the capture
    /// instant, like those of the attribute the source added.
    #[test]
    fn a_merged_local_state_takes_the_capture_instant_in_place_of_its_stored_times() {
        let mut local = state("urn:ngsi-ld:Vehicle:1", SPEED, &json!(10));
        local.stamp_attribute_instants(None, "2020-01-01T00:00:00Z".parse().expect("RFC 3339"));
        let claimed = vec![ForwardedEntities {
            registration_id: Some("urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("a valid URI")),
            mode: ForwardingMode::Inclusive,
            entities: vec![version(state("urn:ngsi-ld:Vehicle:1", BRAND, &json!(4)))],
            warnings: Vec::new(),
            linked_map: None,
        }];
        let captured_at = Utc::now();
        let merged = ForwardedCapture::new(limit()).merged(vec![local], claimed, captured_at).expect("merges");
        let at = Some(SystemInstant::new(captured_at));
        pretty_assertions::assert_eq!(stamps(&merged[0], SPEED), vec![(at, at)]);
        pretty_assertions::assert_eq!(stamps(&merged[0], BRAND), vec![(at, at)]);
    }

    /// Nothing to merge captures nothing.
    #[test]
    fn no_source_entity_captures_nothing() {
        let states = ForwardedCapture::new(limit()).states(Vec::new(), Utc::now()).expect("nothing to capture");
        pretty_assertions::assert_eq!(states.len(), 0);
    }
}
