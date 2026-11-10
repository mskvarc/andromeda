//! What a source answers a forwarded Retrieve Entity with (clauses 5.7.1.4 and 4.5.23).
//!
//! Clause 5.7.1.4 merges what the sources return for the requested entity, so only that entity is taken from an answer. A retrieval answers one Entity: an array, or an entity whose `id` is another, carries nothing that belongs to the requested one, and is dropped with a clause 6.3.17 warning (111, a response received in time whose payload cannot be used) naming the source. A retrieval continuing a clause 4.5.23 flat join answers the requested entity and its Linked Entities side by side; the Linked Entities are attached to the entity they were joined for, never merged into it.

use crate::{
    answer_reading::AnswerDocument,
    forwarded_query::client_query::ClientQuery,
    outcome::ForwardedEntities,
    source_answer_failure::SourceAnswerFailure,
};
use andromeda_linked_retrieval::join::RetrievalShape;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::mem;
use tracing::warn;

/// What a source answers the retrieval `query` describes, whose `join` travels to it. Only a Retrieve Entity is forwarded as a retrieval, and any other query reads as one entity.
pub(crate) const fn shape_of(query: ClientQuery<'_>) -> RetrievalShape {
    match query {
        ClientQuery::EntityRetrieve { params, .. } => RetrievalShape::of(params.join),
        ClientQuery::EntityQuery(_) | ClientQuery::TemporalQuery(_) | ClientQuery::TemporalRetrieve { .. } => RetrievalShape::Entity,
    }
}

/// The document an answer of `shape` is read as. A source not serving the join answers a flat join with the entity alone, so either shape is read for it.
pub(crate) const fn document(shape: RetrievalShape) -> AnswerDocument {
    match shape {
        RetrievalShape::Entity => AnswerDocument::Entity,
        RetrievalShape::EntityBesideLinked => AnswerDocument::Entities,
    }
}

/// Keep of `answer`, an answer of `shape`, only the entity `requested`, the Linked Entities beside it attached to it, and report what does not belong to it.
pub(crate) fn settle(shape: RetrievalShape, requested: &Identifier, answer: &mut ForwardedEntities) {
    let ForwardedEntities {
        registration_id: Some(registration),
        entities,
        warnings,
        ..
    } = answer
    else {
        return;
    };
    let (mut held, others): (Vec<_>, Vec<_>) = mem::take(entities).into_iter().partition(|received| received.state.id() == requested);
    match shape {
        RetrievalShape::Entity => {
            for other in others {
                let answered = other.state.id();
                warn!(%registration, %requested, %answered, "context source answered a retrieval with another entity, which is dropped");
                warnings.push(SourceAnswerFailure::OtherEntity { registration, requested }.into_warning());
            }
        }
        RetrievalShape::EntityBesideLinked => match held.first_mut() {
            Some(entity) => entity.linked.extend(others),
            None if !others.is_empty() => {
                warn!(%registration, %requested, "context source answered a flat join without the entity it was asked for, which is dropped");
                warnings.push(SourceAnswerFailure::RequestedEntityMissing { registration, requested }.into_warning());
            }
            None => {}
        },
    }
    *entities = held;
}

#[cfg(test)]
mod tests {
    use crate::{
        answer_reading::{AnswerDocument, received_answer},
        fixed_registration_store::offline_service,
        forwarded_query::client_query::ClientQuery,
        outcome::{ForwardGetOutcome, ForwardReadOutcome, ForwardedEntities, ForwardedResponse, MatchedSources},
        retrieval_answer::{document, settle, shape_of},
        source_continuation::SourceContinuation,
    };
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_linked_retrieval::join::RetrievalShape;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::entity::GetEntityParams;
    use serde_json::json;
    use std::sync::Arc;

    const REQUESTED: &str = "urn:ngsi-ld:Vehicle:A";

    fn registration() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// What one source answering a retrieval of `REQUESTED` with `body` contributes, settled as `expected`.
    async fn settled(body: &serde_json::Value, expected: RetrievalShape) -> ForwardedEntities {
        let registration = registration();
        let outcome = ForwardGetOutcome::Success(ForwardedResponse {
            body: Arc::from(body.to_string().as_bytes()),
            naming: Box::new(ContextSpec::none()),
            warnings: Vec::new(),
            entity_map: None,
            continuation: SourceContinuation::Complete,
            partial_page: None,
        });
        let page = received_answer(&registration, outcome, document(expected));
        let outcome = ForwardReadOutcome {
            results: page.answer.into_iter().collect(),
            sources: MatchedSources::default(),
        };
        let mut answer = offline_service(Vec::new()).answered_states(outcome).await.results.pop().expect("one answer");
        settle(expected, &REQUESTED.parse::<Identifier>().expect("valid id"), &mut answer);
        answer
    }

    fn ids(answer: &ForwardedEntities) -> Vec<String> {
        answer.entities.iter().map(|received| received.state.id().to_string()).collect()
    }

    fn codes(answer: &ForwardedEntities) -> Vec<WarnCode> {
        answer.warnings.iter().map(NgsiLdWarning::code).collect()
    }

    fn vehicle(id: &str) -> serde_json::Value {
        json!({"id": id, "type": "Vehicle", "speed": {"type": "Property", "value": 1}})
    }

    #[test]
    fn only_a_flat_join_answers_side_by_side() {
        let id: Identifier = REQUESTED.parse().expect("valid id");
        let answer = |raw: &str| {
            let params = serde_urlencoded::from_str::<GetEntityParams>(raw).expect("valid retrieve parameters");
            shape_of(ClientQuery::EntityRetrieve { id: &id, params: &params })
        };
        pretty_assertions::assert_eq!(answer("join=flat&joinLevel=1"), RetrievalShape::EntityBesideLinked);
        pretty_assertions::assert_eq!(answer("join=inline&joinLevel=1"), RetrievalShape::Entity);
        pretty_assertions::assert_eq!(answer("join=@none"), RetrievalShape::Entity);
        pretty_assertions::assert_eq!(answer(""), RetrievalShape::Entity);
    }

    #[tokio::test]
    async fn the_requested_entity_is_kept_without_a_warning() {
        let answer = settled(&vehicle(REQUESTED), RetrievalShape::Entity).await;
        pretty_assertions::assert_eq!(ids(&answer), vec![REQUESTED.to_owned()]);
        assert!(answer.warnings.is_empty(), "{:?}", answer.warnings);
    }

    /// Clause 5.7.1.4 merges what the sources return for the requested entity; another entity is a payload that cannot be used (clause 6.3.17, 111), and the identifier it names is the source's data, which is logged and not reported.
    #[tokio::test]
    async fn another_entity_is_dropped_with_a_warning_naming_the_source() {
        let answer = settled(&vehicle("urn:ngsi-ld:Vehicle:B"), RetrievalShape::Entity).await;
        assert!(answer.entities.is_empty(), "{:?}", ids(&answer));
        pretty_assertions::assert_eq!(codes(&answer), vec![WarnCode::RevalidationFailed]);
        let text = answer.warnings[0].to_string();
        assert!(text.contains("urn:ngsi-ld:ContextSourceRegistration:1") && text.contains(REQUESTED), "{text}");
        assert!(
            !text.contains("urn:ngsi-ld:Vehicle:B"),
            "a warning does not repeat what the source sent: {text}"
        );
    }

    /// A retrieval answers one Entity (clause 5.7.1), so an array is unusable even when it holds the requested entity.
    #[tokio::test]
    async fn an_array_is_dropped_with_a_warning_naming_the_source() {
        let answer = settled(&json!([vehicle(REQUESTED), vehicle("urn:ngsi-ld:Vehicle:B")]), RetrievalShape::Entity).await;
        assert!(answer.entities.is_empty(), "{:?}", ids(&answer));
        pretty_assertions::assert_eq!(codes(&answer), vec![WarnCode::RevalidationFailed]);
        assert!(
            answer.warnings[0].to_string().contains("urn:ngsi-ld:ContextSourceRegistration:1"),
            "{}",
            answer.warnings[0]
        );
    }

    /// Clause 4.5.23: the Linked Entities of a flat join stand beside the entity, and are attached to it rather than merged into it.
    #[tokio::test]
    async fn a_flat_join_attaches_the_linked_entities_to_the_requested_one() {
        let answer = settled(
            &json!([vehicle("urn:ngsi-ld:Vehicle:B"), vehicle(REQUESTED)]),
            RetrievalShape::EntityBesideLinked,
        )
        .await;
        pretty_assertions::assert_eq!(ids(&answer), vec![REQUESTED.to_owned()]);
        let linked: Vec<String> = answer.entities[0].linked.iter().map(|received| received.state.id().to_string()).collect();
        pretty_assertions::assert_eq!(linked, vec!["urn:ngsi-ld:Vehicle:B".to_owned()]);
        assert!(answer.warnings.is_empty(), "{:?}", answer.warnings);
    }

    #[tokio::test]
    async fn a_flat_join_without_the_requested_entity_is_dropped_with_a_warning() {
        let answer = settled(&json!([vehicle("urn:ngsi-ld:Vehicle:B")]), RetrievalShape::EntityBesideLinked).await;
        assert!(answer.entities.is_empty(), "{:?}", ids(&answer));
        pretty_assertions::assert_eq!(codes(&answer), vec![WarnCode::RevalidationFailed]);
    }

    #[tokio::test]
    async fn a_flat_join_answered_with_the_entity_alone_keeps_it() {
        let answer = settled(&vehicle(REQUESTED), RetrievalShape::EntityBesideLinked).await;
        pretty_assertions::assert_eq!(ids(&answer), vec![REQUESTED.to_owned()]);
        assert!(answer.warnings.is_empty(), "{:?}", answer.warnings);
    }

    #[test]
    fn a_retrieval_reads_one_object_and_a_flat_join_either_shape() {
        pretty_assertions::assert_eq!(document(RetrievalShape::Entity), AnswerDocument::Entity);
        pretty_assertions::assert_eq!(document(RetrievalShape::EntityBesideLinked), AnswerDocument::Entities);
    }
}
