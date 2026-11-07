//! What a distributed Temporal Evolution write hands to the registrations it
//! matches and what it keeps for the local phase (clauses 5.6.11.4, 5.6.12.4
//! and 5.6.14.4).
//!
//! The registrations read the written evolution as the document it expands to
//! under the request's context, so what they claim (clause 4.3.6.3) is named by
//! expanded names; the local phase keeps writing the compact payload, from
//! which the claimed attributes are taken out by the expanded name each of its
//! member names stands for.

use crate::{error::TemporalError, service::TemporalService, written_evolution::expand_reified_scope};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;

/// A written Temporal Evolution as the registrations it is forwarded to read it.
#[derive(Debug)]
pub struct ForwardableEvolution {
    /// The instances, Scope paths and identity the evolution writes.
    document: EntityDocument,
    /// The reified Scope instances (clause 4.5.6), keyed by the Scope IRI,
    /// when the body dates its Scope.
    scope_history: Option<EntityDocument>,
}

impl ForwardableEvolution {
    /// The expanded document of the instances and Scope paths.
    #[must_use]
    pub const fn document(&self) -> &EntityDocument {
        &self.document
    }

    /// The expanded reified Scope instances, when the body carries them.
    #[must_use]
    pub const fn scope_history(&self) -> Option<&EntityDocument> {
        self.scope_history.as_ref()
    }
}

impl TemporalService {
    /// The evolution `written` expands to under `context`, which the
    /// registrations a write is forwarded to read.
    ///
    /// A reified Scope (clause 4.5.6) is the Temporal Evolution of the Scope,
    /// so its instances, with their temporal sub-properties, are expanded as
    /// the local write expands them and forwarded as received (clause 5.6.11.4:
    /// the matching input data).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the context does not resolve or
    /// the processor refuses the payload, and [`TemporalError::Validation`]
    /// when the payload is no Temporal Evolution the model admits.
    pub async fn forwardable_document(&self, written: &WrittenTemporalEntity, context: &ContextSpec) -> Result<ForwardableEvolution, TemporalError> {
        let expansion = EntityExpansion::prepare(&self.engine, context)
            .await?
            .bounded_by(self.limits.payload_budget.json_values);
        let (fragment, reified) = written.clone().into_fragment(); // clone: the local phase still writes the payload
        let document = expansion.entity_patch(&fragment, Normalization::Entity).await?;
        let scope_history = match reified {
            Some(instances) => Some(expand_reified_scope(&expansion, instances).await?),
            None => None,
        };
        Ok(ForwardableEvolution { document, scope_history })
    }

    /// `written` without the attributes proxied registrations claimed
    /// (clause 4.3.6.3), named by their expanded names: "any remaining input
    /// data" of the local phase.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when a member name does not expand
    /// under `context`.
    pub async fn without_claimed(
        &self,
        mut written: WrittenTemporalEntity,
        claimed: &[AttributeName],
        context: &ContextSpec,
    ) -> Result<WrittenTemporalEntity, TemporalError> {
        if claimed.is_empty() {
            return Ok(written);
        }
        let terms = self.engine.prepare_term_context(context);
        let mut taken = Vec::new();
        for member in written.attrs.keys() {
            let expanded = self.engine.expand_term(member.as_str(), &terms).await?;
            if claimed.iter().any(|name| name.as_str() == expanded.as_str()) {
                taken.push(member.clone()); // clone: the strip names the members the map is keyed by
            }
        }
        written.strip_attributes(&taken);
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalError,
        reified_scope_fixture::{admitted_scope, evolution_with_scope, refused_scopes},
        stub_broker::{StubBroker, service},
        temporal_validation_reason::TemporalValidationReason,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde_json::json;

    fn evolution() -> WrittenTemporalEntity {
        let text = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"},
                {"type": "Property", "value": 2, "observedAt": "2025-01-02T00:00:00Z"}
            ],
            "brand": [{"type": "Property", "value": "x", "observedAt": "2025-01-01T00:00:00Z"}]
        })
        .to_string();
        WrittenTemporalEntity::from_request_text(text.as_bytes(), ContextReading::keyword_spellings()).expect("a written evolution")
    }

    fn core(name: &str) -> AttributeName {
        format!("https://uri.etsi.org/ngsi-ld/default-context/{name}")
            .parse()
            .expect("a valid attribute name")
    }

    /// The registrations a write is forwarded to read the evolution's
    /// instances by expanded name.
    #[tokio::test]
    async fn a_written_evolution_expands_to_its_instances_by_expanded_name() {
        let service = service(StubBroker::new([]));
        let document = service
            .forwardable_document(&evolution(), &ContextSpec::core_only())
            .await
            .expect("the evolution expands");
        let mut names = document.document().attributes().keys().cloned().collect::<Vec<_>>();
        names.sort();
        pretty_assertions::assert_eq!(names, vec![core("brand"), core("speed")]);
        pretty_assertions::assert_eq!(document.document().attributes()[&core("speed")].instances().len(), 2);
        assert!(document.scope_history().is_none());
    }

    /// Clauses 4.5.6 and 5.6.11.4: both instances of a reified Scope, the
    /// ended one included, are forwarded with their temporal sub-properties.
    #[tokio::test]
    async fn a_reified_scope_is_forwarded_as_both_its_instances() {
        let written = evolution_with_scope(&admitted_scope());
        let service = service(StubBroker::new([]));
        let forwardable = service
            .forwardable_document(&written, &ContextSpec::core_only())
            .await
            .expect("the evolution expands");
        let history = forwardable.scope_history().expect("the Scope history is carried");
        let scope = history
            .attributes()
            .get("https://uri.etsi.org/ngsi-ld/scope")
            .expect("held under the Scope IRI");
        pretty_assertions::assert_eq!(scope.instances().len(), 2);
        assert!(forwardable.document().scope().is_none());
        pretty_assertions::assert_eq!(forwardable.document().attributes().len(), 1);
    }

    /// Clause 4.5.6: a reified Scope carrying a member beyond the four
    /// Temporal Properties is refused before it is forwarded, answered with
    /// `BadRequestData` (clause 5.5.4).
    #[tokio::test]
    async fn a_reified_scope_carrying_a_forbidden_member_is_refused_before_it_is_forwarded() {
        let service = service(StubBroker::new([]));
        for (scope, expected) in refused_scopes() {
            let refused = service
                .forwardable_document(&evolution_with_scope(&scope), &ContextSpec::core_only())
                .await
                .expect_err("the reified Scope carries a forbidden member");
            assert!(
                matches!(&refused, TemporalError::Validation(TemporalValidationReason::Representation(EntityRefusal::Scope(refusal))) if *refusal == expected),
                "{scope}: {refused:?}"
            );
        }
    }

    /// A Scope written as paths stays on the document, and no
    /// history is carried.
    #[tokio::test]
    async fn a_scope_of_paths_stays_on_the_forwarded_document() {
        let service = service(StubBroker::new([]));
        let forwardable = service
            .forwardable_document(&evolution_with_scope(&json!("/Madrid")), &ContextSpec::core_only())
            .await
            .expect("the evolution expands");
        assert!(forwardable.scope_history().is_none());
        assert!(forwardable.document().scope().is_some());
    }

    /// Clause 5.6.11.4: the local phase keeps what proxied registrations did
    /// not claim, named by expanded names.
    #[tokio::test]
    async fn the_attributes_a_proxied_registration_claimed_leave_the_local_evolution() {
        let service = service(StubBroker::new([]));
        let remainder = service
            .without_claimed(evolution(), &[core("speed")], &ContextSpec::core_only())
            .await
            .expect("the claim is taken out");
        pretty_assertions::assert_eq!(remainder.attrs.keys().map(ToString::to_string).collect::<Vec<_>>(), vec!["brand"]);
        let untouched = service
            .without_claimed(evolution(), &[], &ContextSpec::core_only())
            .await
            .expect("nothing is taken out");
        pretty_assertions::assert_eq!(untouched.attrs.len(), 2);
    }
}
