//! Which written Temporal Evolution payloads a write path admits.
//!
//! ETSI GS CIM 009 v1.9.1 clauses 5.6.11.4 and 5.6.12.4 both open with "Execute
//! the behaviour defined in clause 5.5.4 on JSON-LD validation" and forward to
//! matching registrations only afterwards, so the screening runs here, ahead of
//! the forwarding phase: a payload this broker refuses reaches no Context
//! Source, and a write whose attributes a proxied registration claims in full
//! is validated rather than answered 204 unexamined.
//!
//! What is screened is the half of clause 5.5.4 decidable on the compact
//! payload a client wrote: the attribute types of clause 5.2.4, and the rule
//! that `entityIdSealed` and `entityTypeSealed` belong to the `ngsildproof`
//! Property of clause 4.5.2.2 alone -- which clauses 4.5.3.2 and 4.5.3.3
//! sharpen on a Relationship, in every representation, into "'entityIdSealed'
//! and 'entityTypeSealed' shall never be present". The seals are declared only
//! inside `ngsildproof`'s own scoped context in the core `@context`, so after
//! expansion a forged one is indistinguishable from a sub-attribute and no
//! later stage can refuse it. Clause 5.6.14.4 does not name clause 5.5.4, but
//! the prohibition it enforces is a statement about the representation and not
//! about one operation, so an instance modified through clause 5.6.14 is held
//! to it too.
//!
//! A Fragment addressed by a request path is also held to clause 5.4: its
//! `id`, which the path makes optional, "shall be equal to the id of the target
//! (mutated) NGSI-LD element". Clauses 5.6.12 and 5.6.14 both take such a
//! Fragment, and a mismatch is refused here so that no Context Source receives
//! a Fragment for another Entity.
//!
//! Clause 5.5.7's two prohibitions on the user `@context` are screened here for
//! the same reason and at the same point: a `@context` embedded below the
//! document's top level is decidable on the payload, and whether the request's
//! own contexts declare a JSON-LD Scoped Context is decidable once they are
//! resolved. Both "should result in an error of type `BadRequestData`", and
//! both have to be decided before the forwarding phase, because a payload or a
//! context this broker refuses must reach no Context Source. The same walk
//! refuses a key JSON-LD expansion would drop or rewrite past recovery, which
//! clause 4.6.4 forbids losing.
//!
//! The NGSI-LD Null is deliberately not screened here. Clause 5.5.4's second
//! bullet excepts from its prohibition "the representation of deleted
//! Properties, Relationships or Language Properties in notifications and the
//! temporal evolution", and clause 4.18 says the same of a deleted scope. A
//! Temporal Evolution is exactly where clause 4.5.0's deletions are recorded
//! and served back -- clause 5.6.6 leaves the evolution standing when the
//! current entity is deleted, and annex C, clause C.5.16 shows the deleted
//! scope in the temporal representation -- so refusing the sentinel in the
//! value position of a temporal write would refuse the representation the
//! clause requires this broker to hold. The two bullets that do bind a temporal
//! payload -- the first level member value and the key nested inside a Property
//! value object, neither of which the temporal exception covers -- are recorded
//! as a follow-up in `docs/spec-conformance-sweep.md` rather than screened,
//! because reaching them needs the expanded payload the temporal write path
//! does not build before it forwards.

use crate::request_context_admission::admit_request_context;
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_representation::{attribute_type_validation::validate_fragment_attribute_types, compact_key_admission::screen_keys_in_fragment};
use andromeda_ngsi_ld_request::{fragment_entity_id::admit_fragment_entity_id, temporal_instance_fragment::admit_temporal_instance_fragment};
use axum::http::Uri as HttpUri;
use serde_json::Value;
use std::fmt::Display;

/// Screen one written `EntityTemporal` payload (clauses 5.6.11 and 5.6.12)
/// before it is forwarded or stored.
///
/// # Errors
///
/// Answers `BadRequestData` for the first attribute node clauses 5.2.4 and
/// 4.5.2.2 do not admit, for a user `@context` clause 5.5.7 refuses, for a
/// `@context` embedded below the document's top level, and for a key clause
/// 4.6.4 refuses (see `compact_key_admission`); a context the request
/// names that does not resolve is answered `LdContextNotAvailable` instead.
pub(crate) async fn admit_written_evolution(engine: &JsonLdEngine, payload: &Value, context: &ContextSpec, instance: &HttpUri) -> Result<(), ProblemResponse> {
    admit_request_context(engine, context, instance).await?;
    validate_fragment_attribute_types(payload).map_err(|error| bad_request_data(&error, instance))?;
    screen_keys_in_fragment(payload).map_err(|error| bad_request_data(&error, instance))
}

/// Screen the `EntityTemporal` Fragment a clause 5.6.12 addition to `entity`
/// carries: its `id`, when present, first, then the same checks as any written
/// `EntityTemporal` payload.
///
/// # Errors
///
/// Answers `BadRequestData` for a Fragment whose `id` is not `entity` (clause
/// 5.4), and otherwise on the same terms as [`admit_written_evolution`].
pub(crate) async fn admit_added_attributes(
    engine: &JsonLdEngine,
    entity: &Identifier,
    payload: &Value,
    context: &ContextSpec,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    // A payload that is no JSON object names no `id`; the checks below and the
    // expansion refuse it on their own terms.
    if let Value::Object(members) = payload {
        admit_fragment_entity_id(members, entity).map_err(|error| bad_request_data(&error, instance))?;
    }
    admit_written_evolution(engine, payload, context, instance).await
}

/// Screen the `EntityTemporal` Fragment a clause 5.6.14 modification of `entity`
/// carries: its clause 5.6.14.3 shape first, then the same checks as any written
/// `EntityTemporal` payload.
///
/// # Errors
///
/// Answers `BadRequestData` for a body that is not a Fragment naming exactly
/// one Attribute with an Array of exactly one instance, or whose `id` is not
/// `entity` (clause 5.4), and otherwise on the same terms as
/// [`admit_written_evolution`].
pub(crate) async fn admit_written_instance(
    engine: &JsonLdEngine,
    entity: &Identifier,
    payload: &Value,
    context: &ContextSpec,
    instance: &HttpUri,
) -> Result<(), ProblemResponse> {
    admit_temporal_instance_fragment(payload, entity).map_err(|error| bad_request_data(&error, instance))?;
    admit_written_evolution(engine, payload, context, instance).await
}

/// The clause 5.5.4 answer to a representation this broker refuses.
fn bad_request_data<E: Display>(error: &E, instance: &HttpUri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()).with_instance(instance.clone())
    // clone: the problem details own the instance the borrowed request names
}

#[cfg(test)]
mod tests {
    use crate::written_evolution_admission::{admit_added_attributes, admit_written_evolution, admit_written_instance};
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::http::{StatusCode, Uri as HttpUri};
    use serde_json::{Value, json};

    fn instance() -> HttpUri {
        "/ngsi-ld/v1/temporal/entities".parse().expect("a valid request target")
    }

    fn entity() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity identifier")
    }

    /// The clause 5.6.14.3 `EntityTemporal` Fragment carrying `instance` as the
    /// one instance of `belongsTo`.
    fn fragment(instance: &Value) -> Value {
        json!({"belongsTo": [instance]})
    }

    /// An engine that resolves the embedded core context and nothing else.
    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// Clauses 4.5.3.2 and 4.5.3.3, "Prohibited": on a Relationship, in either
    /// representation, "'entityIdSealed' and 'entityTypeSealed' shall never be
    /// present" -- including in the instances of a Temporal Evolution.
    #[tokio::test]
    async fn a_forged_seal_on_a_temporal_relationship_instance_is_refused() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let payload = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "belongsTo": [
                    {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "observedAt": "2024-01-01T00:00:00Z"},
                    {"type": "Relationship", "object": "urn:ngsi-ld:Site:2", "observedAt": "2024-01-02T00:00:00Z", member: "urn:ngsi-ld:Vehicle:1"}
                ]
            });
            assert!(
                admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_err(),
                "{member}"
            );
        }
    }

    /// The same seal forged on the single instance a clause 5.6.14
    /// modification carries.
    #[tokio::test]
    async fn a_forged_seal_on_a_modified_instance_is_refused() {
        let payload = fragment(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Vehicle"}));
        assert!(
            admit_written_instance(&engine(), &entity(), &payload, &ContextSpec::none(), &instance())
                .await
                .is_err()
        );
    }

    /// An ordinary Temporal Evolution passes, instances and all.
    #[tokio::test]
    async fn a_temporal_evolution_of_ordinary_instances_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": 20, "observedAt": "2024-01-02T00:00:00Z"}
            ]
        });
        assert!(admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_ok());
        assert!(
            admit_written_instance(
                &engine(),
                &entity(),
                &fragment(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"})),
                &ContextSpec::none(),
                &instance()
            )
            .await
            .is_ok()
        );
    }

    /// Clause 5.6.14.3 takes an `EntityTemporal` Fragment; the bare instance it
    /// would contain is refused before anything is forwarded or stored.
    #[tokio::test]
    async fn a_bare_modified_instance_is_refused() {
        let bare = json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"});
        assert!(
            admit_written_instance(&engine(), &entity(), &bare, &ContextSpec::none(), &instance())
                .await
                .is_err()
        );
    }

    /// The clause 5.6.12.3 Fragment adding a `speed` instance, carrying `id`
    /// when one is given.
    fn added_attributes(id: Option<&str>) -> Value {
        let mut payload = json!({
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}]
        });
        if let (Some(id), Some(members)) = (id, payload.as_object_mut()) {
            members.insert("id".to_owned(), json!(id));
        }
        payload
    }

    /// Clause 5.4: a Fragment's `id` "shall be equal to the id of the target
    /// (mutated) NGSI-LD element"; one naming another Entity is data clause
    /// 5.5.4 answers with `BadRequestData`, before anything is forwarded.
    #[tokio::test]
    async fn added_attributes_naming_another_entity_are_refused() {
        let refusal = admit_added_attributes(
            &engine(),
            &entity(),
            &added_attributes(Some("urn:ngsi-ld:Vehicle:2")),
            &ContextSpec::none(),
            &instance(),
        )
        .await
        .expect_err("a Fragment naming another Entity is refused");
        pretty_assertions::assert_eq!(refusal.problem().error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(refusal.problem().status_code(), StatusCode::BAD_REQUEST);
    }

    /// Clause 5.4: a Fragment naming its target, or naming none where the
    /// request path does, is admitted.
    #[tokio::test]
    async fn added_attributes_naming_the_target_or_no_id_are_admitted() {
        for id in [Some("urn:ngsi-ld:Vehicle:1"), None] {
            assert!(
                admit_added_attributes(&engine(), &entity(), &added_attributes(id), &ContextSpec::none(), &instance())
                    .await
                    .is_ok(),
                "{id:?}"
            );
        }
    }

    /// Clause 5.5.4's second bullet excepts "the representation of deleted
    /// Properties, Relationships or Language Properties in notifications and
    /// the temporal evolution", so an instance recording a deletion is data a
    /// Temporal Evolution holds rather than a payload to refuse.
    #[tokio::test]
    async fn an_instance_recording_a_deletion_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2024-01-02T00:00:00Z"}]
        });
        assert!(admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_ok());
    }

    /// Clause 5.2.4 fixes the attribute types, so a token outside the eight is
    /// not an attribute wherever it is written.
    #[tokio::test]
    async fn an_instance_declaring_a_token_outside_clause_5_2_4_is_refused() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "NotAnAttributeType", "value": 10}]
        });
        assert!(admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_err());
    }

    /// Clause 5.5.7: "there cannot be parts of the user @context other than at
    /// the top level of the NGSI-LD document".
    #[tokio::test]
    async fn a_context_embedded_in_a_temporal_instance_is_refused() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z", "@context": {"speed": "https://example.org/speed"}}]
        });
        assert!(admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_err());

        let modified = fragment(&json!({"type": "Property", "value": 10, "unit": {"@context": {}}}));
        assert!(
            admit_written_instance(&engine(), &entity(), &modified, &ContextSpec::none(), &instance())
                .await
                .is_err()
        );
    }

    /// Clause 5.5.7: the user `@context` "shall not contain JSON-LD Scoped
    /// Contexts", whichever operation carries it.
    #[tokio::test]
    async fn a_scoped_user_context_is_refused_on_the_temporal_write_path() {
        let context = ContextSpec::from_body(json!({
            "speed": {"@id": "https://example.org/speed", "@context": {"observedAt": "https://example.org/rewritten"}}
        }));
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"}]
        });
        assert!(admit_written_evolution(&engine(), &payload, &context, &instance()).await.is_err());
        assert!(
            admit_written_instance(
                &engine(),
                &entity(),
                &fragment(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"})),
                &context,
                &instance()
            )
            .await
            .is_err()
        );
    }

    /// The `json` element of a `JsonProperty` holds native JSON, so a key
    /// spelled `@context` inside one is data (clause 5.5.7, closing note).
    #[tokio::test]
    async fn a_context_key_inside_a_temporal_json_property_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "config": [{"type": "JsonProperty", "json": {"@context": "https://example.org/not-a-context"}, "observedAt": "2024-01-01T00:00:00Z"}]
        });
        assert!(admit_written_evolution(&engine(), &payload, &ContextSpec::none(), &instance()).await.is_ok());
    }
}
