//! One entity of an inbound notification, read into the model under the
//! context it was written in (clause 5.8.6).
//!
//! The sender spelled its entity under the context of the copy it holds -- the
//! notification's own, or the `@context` an `application/ld+json` notification
//! writes on each entity -- so the entity is expanded once under that context
//! into the written document the local filters read and the local
//! subscription's compaction writes. A written document, unlike an entity
//! state, holds the `urn:ngsi-ld:null` a source notifies a deleted attribute
//! with: "only the name of the attribute as key and the URI "urn:ngsi-ld:null"
//! as value" report a deleted attribute of any family, so a first-level Null
//! stays the deletion of the member, and is relayed as the bare Null it arrived
//! as.
//!
//! Completing a split entity merges on the model: the deletions the sender
//! notified are set aside, the rest of its part becomes the version the local
//! copy and every other holder fold into (clause 4.5.5), and the deletions are
//! put back on the completed entity, which no other holder's instance
//! overrides.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{
    document::{EntityDocument, attribute_member::AttributeMember},
    entity_state::EntityState,
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_expansion::{
    entity_expansion::EntityExpansion,
    error::ExpansionError,
    received_state::{ReceivedDocument, ReceivedRefusal, ReceivedState},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::received::ContextualEntity;
use thiserror::Error;

/// Why one entity of an inbound notification is not re-assembled.
#[derive(Debug, Error)]
pub(crate) enum ReceivedEntityRefusal {
    /// The entity's own `@context` does not resolve or process.
    #[error("its own @context cannot be processed: {0}")]
    OwnContext(#[source] ExpansionError),
    /// The entity does not expand under the context it was written in.
    #[error(transparent)]
    Expansion(#[from] ReceivedRefusal),
    /// The part the sender notified is no entity a holder's version can be
    /// merged onto.
    #[error("the notified part cannot be completed: {0}")]
    Incomplete(#[source] EntityRefusal),
}

/// Read one entity of an inbound notification under its own `@context` when
/// it names one (JSON-LD 1.1: a node's context applies to that node), else
/// under the notification's, `inbound`.
///
/// # Errors
///
/// Returns [`ReceivedEntityRefusal`] when the entity's own context does not
/// process or the entity does not expand.
pub(crate) async fn read_notified(
    engine: &JsonLdEngine,
    inbound: &EntityExpansion<'_>,
    contextual: &ContextualEntity,
) -> Result<ReceivedDocument, ReceivedEntityRefusal> {
    match &contextual.context {
        None => Ok(inbound.notified(&contextual.entity).await?),
        Some(own) => {
            // clone: the context specification owns the literal it names
            let expansion = EntityExpansion::prepare(engine, &ContextSpec::from_body(own.clone()))
                .await
                .map_err(ReceivedEntityRefusal::OwnContext)?;
            Ok(expansion.notified(&contextual.entity).await?)
        }
    }
}

/// The notified part of a split entity, set apart into the version its other
/// holders' versions merge onto and the deletions it notified.
#[derive(Debug)]
pub(crate) struct NotifiedPart {
    /// The part as a version of the entity.
    pub(crate) version: ReceivedState,
    /// The attributes the sender notified deleted, in notified order.
    pub(crate) deletions: Vec<AttributeName>,
}

impl NotifiedPart {
    /// Set `part` apart.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedEntityRefusal::Incomplete`] for a part that, its
    /// deletions set aside, is no entity: one that deletes its `scope` or an
    /// instance by its family's deletion encoding.
    pub(crate) fn of(part: ReceivedDocument) -> Result<NotifiedPart, ReceivedEntityRefusal> {
        let ReceivedDocument { document, system_times } = part;
        let parts = document.into_parts();
        let mut deletions = Vec::new();
        let mut kept = parts.attributes;
        kept.retain(|name, member| match member {
            AttributeMember::Remove => {
                deletions.push(name.clone()); // clone: the deletion is put back after the merge
                false
            }
            AttributeMember::Instances(_) => true,
        });
        let state = EntityState::try_from(EntityDocument::new(parts.id, parts.types, parts.scope, parts.expires_at, kept))
            .map_err(ReceivedEntityRefusal::Incomplete)?;
        Ok(NotifiedPart {
            version: ReceivedState {
                state,
                system_times,
                linked: Vec::new(),
            },
            deletions,
        })
    }

    /// The completed entity `merged` is, as the written document the local
    /// filters read, the notified deletions put back.
    pub(crate) fn completed(merged: ReceivedState, deletions: Vec<AttributeName>) -> ReceivedDocument {
        let ReceivedState { mut state, system_times, .. } = merged;
        state.attributes_mut().retain(|name, _| !deletions.contains(name));
        let mut document = EntityDocument::from(&state);
        for deletion in deletions {
            document.attributes_mut().insert(deletion, AttributeMember::Remove);
        }
        ReceivedDocument { document, system_times }
    }
}

#[cfg(test)]
mod tests {
    use crate::received_entity::{NotifiedPart, read_notified};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
    use andromeda_ngsi_ld_entity::document::attribute_member::AttributeMember;
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_model::received::{ReceivedElement, ReceivedEntities};
    use andromeda_ngsi_ld_query::projection::compiled::Projection;
    use andromeda_ngsi_ld_render::{
        notification_entity::NotificationEntity,
        notification_language::NotificationLanguage,
        structural_projection::StructuralProjection,
    };
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// `entity` read as a source notified it and written for a local
    /// subscriber in `format`, both under the core context alone.
    async fn relayed(entity: &Value, format: ResponseFormat) -> Value {
        relayed_under(entity, format, None).await
    }

    /// `entity` relayed in `format` for a subscription naming `language`.
    async fn relayed_under(entity: &Value, format: ResponseFormat, language: Option<&NotificationLanguage>) -> Value {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let inbound = EntityExpansion::prepare(&engine, &context).await.expect("the core context processes");
        let elements = ReceivedEntities::from_slice(entity.to_string().as_bytes()).expect("an entity");
        let [ReceivedElement::Read(contextual)] = elements.0.as_slice() else {
            panic!("the notified entity reads");
        };
        let received = read_notified(&engine, &inbound, contextual).await.expect("the notified entity expands");
        let context_compactor = engine.compactor(&context);
        let respelled = EntityCompactor::new(&context_compactor)
            .document_entity(&received.document)
            .await
            .expect("the entity compacts");
        let projection = Projection::default();
        serde_json::to_value(NotificationEntity::relayed(
            &respelled,
            format,
            StructuralProjection::from(&projection),
            language,
        ))
        .expect("the entity writes")
    }

    /// Table 5.2.12-1 `lang` and clause 4.15: a relayed `LanguageProperty` is
    /// reduced to the subscription's language, in every representation that
    /// writes it, and is untouched without one.
    #[tokio::test]
    async fn a_relayed_language_property_is_reduced_under_the_subscription_lang() {
        let notified = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}}
        });
        let english = NotificationLanguage::of(&"en".parse().expect("a language tag"));
        let reduced = relayed_under(&notified, ResponseFormat::Normalized, Some(&english)).await;
        pretty_assertions::assert_eq!(reduced["name"], json!({"type": "Property", "value": "Tower", "lang": "en"}), "{reduced}");
        let simplified = relayed_under(&notified, ResponseFormat::Simplified, Some(&english)).await;
        pretty_assertions::assert_eq!(simplified["name"], json!("Tower"), "{simplified}");
        let untouched = relayed(&notified, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(untouched["name"]["type"], json!("LanguageProperty"), "{untouched}");
    }

    /// Clause 5.8.6: a source reports a deleted attribute of any family as
    /// "only the name of the attribute as key and the URI "urn:ngsi-ld:null"
    /// as value", and the relayed copy reports it the same way in every
    /// representation, never as a Property whose value is the Null.
    #[tokio::test]
    async fn a_notified_deletion_is_relayed_as_the_bare_null() {
        let notified = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "owner": "urn:ngsi-ld:null",
            "speed": {"type": "Property", "value": 5}
        });
        for format in [ResponseFormat::Normalized, ResponseFormat::Concise, ResponseFormat::Simplified] {
            let relayed = relayed(&notified, format).await;
            pretty_assertions::assert_eq!(relayed["owner"], json!("urn:ngsi-ld:null"), "{format:?}: {relayed}");
        }
        let normalized = relayed(&notified, ResponseFormat::Normalized).await;
        pretty_assertions::assert_eq!(normalized["speed"], json!({"type": "Property", "value": 5}));
    }

    /// JSON-LD 1.1 with clause 5.8.6: an entity that carries its own `@context`
    /// is read under it, and the local subscription writes the entity under its
    /// own context, so a name the sender aliased differently is the local name.
    #[tokio::test]
    async fn received_entities_are_read_under_their_own_context() {
        let engine = offline_engine();
        let inbound = EntityExpansion::prepare(&engine, &ContextSpec::core_only())
            .await
            .expect("the core context processes");
        let elements = ReceivedEntities::from_slice(
            json!({
                "@context": {"spd": "https://example.org/vocab/speed"},
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "spd": {"type": "Property", "value": 5}
            })
            .to_string()
            .as_bytes(),
        )
        .expect("an entity");
        let [ReceivedElement::Read(contextual)] = elements.0.as_slice() else {
            panic!("the notified entity reads");
        };
        let received = read_notified(&engine, &inbound, contextual).await.expect("the notified entity expands");

        let local = ContextSpec::from_body(JsonLiteral::admit(r#"{"speed": "https://example.org/vocab/speed"}"#).expect("a context"));
        let local_compactor = engine.compactor(&local);
        let respelled = EntityCompactor::new(&local_compactor)
            .document_entity(&received.document)
            .await
            .expect("the entity compacts");
        let projection = Projection::default();
        let written = serde_json::to_value(NotificationEntity::relayed(
            &respelled,
            ResponseFormat::Normalized,
            StructuralProjection::from(&projection),
            None,
        ))
        .expect("the entity writes");

        pretty_assertions::assert_eq!(written["speed"], json!({"type": "Property", "value": 5}), "{written}");
        assert!(written.get("spd").is_none(), "{written}");
    }

    /// A split part sets its notified deletions apart and puts them back on
    /// the completed entity, where no holder's version overrides them.
    #[tokio::test]
    async fn notified_deletions_survive_the_completion() {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let inbound = EntityExpansion::prepare(&engine, &context).await.expect("the core context processes");
        let elements = ReceivedEntities::from_slice(
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "owner": "urn:ngsi-ld:null", "speed": {"type": "Property", "value": 5}})
                .to_string()
                .as_bytes(),
        )
        .expect("an entity");
        let [ReceivedElement::Read(contextual)] = elements.0.as_slice() else {
            panic!("the notified entity reads");
        };
        let part = NotifiedPart::of(read_notified(&engine, &inbound, contextual).await.expect("expands")).expect("a version");
        pretty_assertions::assert_eq!(part.deletions.len(), 1);
        let completed = NotifiedPart::completed(part.version, part.deletions);
        let owner = completed
            .document
            .attributes()
            .iter()
            .find(|(name, _)| name.as_str().ends_with("owner"))
            .map(|(_, member)| member);
        pretty_assertions::assert_eq!(owner, Some(&AttributeMember::Remove));
    }
}
