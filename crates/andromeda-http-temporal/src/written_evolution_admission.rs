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
//! payload a client wrote. The decode of the request holds two rules, so a
//! decoded payload satisfies them: the attribute types of clause 5.2.4, and the
//! rule that `entityIdSealed` and `entityTypeSealed` belong to the `ngsildproof`
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
//! A key JSON-LD expansion would drop or rewrite past recovery, which clause
//! 4.6.4 forbids losing, is refused here for the same reason and at the same
//! point, before the forwarding phase, because a payload this broker refuses
//! must reach no Context Source. Clause 5.5.7's two prohibitions on the user
//! `@context` -- no JSON-LD Scoped Context, and no part of it below the
//! document's top level -- are the request's: the user `@context` admission
//! (`crate::request_context_admission`) refuses both before this runs.
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
//! value object, neither of which the temporal exception covers -- are held by
//! the temporal write, which reads the expanded payload.

use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::{fragment_entity_id::admit_fragment_entity_id, temporal_instance_fragment::admit_temporal_instance_fragment};
use axum::http::Uri as HttpUri;
use std::fmt::Display;

/// Screen one written `EntityTemporal` payload (clauses 5.6.11 and 5.6.12)
/// before it is forwarded or stored.
///
/// # Errors
///
/// Answers `BadRequestData` for a key clause 4.6.4 refuses (see
/// `WrittenTemporalEntity::screen_keys`). The attribute types and the seals
/// were refused when the payload was decoded, and the request's `@context` was
/// admitted before the payload is read (see `crate::request_context_admission`).
pub(crate) fn admit_written_evolution(payload: &WrittenTemporalEntity, instance: &HttpUri) -> Result<(), ProblemResponse> {
    payload.screen_keys().map_err(|error| bad_request_data(&error, instance))
}

/// Screen the `EntityTemporal` Fragment a clause 5.6.12 addition to `entity`
/// carries: its `id`, when present, first, then the same checks as any written
/// `EntityTemporal` payload.
///
/// # Errors
///
/// Answers `BadRequestData` for a Fragment whose `id` is not `entity` (clause
/// 5.4), and otherwise on the same terms as [`admit_written_evolution`].
pub(crate) fn admit_added_attributes(entity: &Identifier, payload: &WrittenTemporalEntity, instance: &HttpUri) -> Result<(), ProblemResponse> {
    admit_fragment_entity_id(payload.id.as_ref(), entity).map_err(|error| bad_request_data(&error, instance))?;
    admit_written_evolution(payload, instance)
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
pub(crate) fn admit_written_instance(entity: &Identifier, payload: &WrittenTemporalEntity, instance: &HttpUri) -> Result<(), ProblemResponse> {
    admit_temporal_instance_fragment(payload, entity).map_err(|error| bad_request_data(&error, instance))?;
    admit_written_evolution(payload, instance)
}

/// The clause 5.5.4 answer to a representation this broker refuses.
fn bad_request_data<E: Display>(error: &E, instance: &HttpUri) -> ProblemResponse {
    ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()).with_instance(instance.clone())
    // clone: the problem details own the instance the borrowed request names
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::written_evolution_admission::{admit_added_attributes, admit_written_evolution, admit_written_instance};
    use andromeda_http_response::problem_response::ProblemResponse;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
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

    /// What the request is refused for, decoding included: a payload the decode
    /// refuses never reaches the admission.
    fn refused(payload: &Value, admit: impl FnOnce(&WrittenTemporalEntity) -> Result<(), ProblemResponse>) -> bool {
        match WrittenTemporalEntity::from_request_text(payload.to_string().as_bytes(), ContextReading::keyword_spellings()) {
            Ok(decoded) => admit(&decoded).is_err(),
            Err(_) => true,
        }
    }

    fn evolution_refused(payload: &Value) -> bool {
        refused(payload, |decoded| admit_written_evolution(decoded, &instance()))
    }

    /// Clauses 4.5.3.2 and 4.5.3.3, "Prohibited": on a Relationship, in either
    /// representation, "'entityIdSealed' and 'entityTypeSealed' shall never be
    /// present" -- including in the instances of a Temporal Evolution.
    #[test]
    fn a_forged_seal_on_a_temporal_relationship_instance_is_refused() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let payload = json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "belongsTo": [
                    {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "observedAt": "2024-01-01T00:00:00Z"},
                    {"type": "Relationship", "object": "urn:ngsi-ld:Site:2", "observedAt": "2024-01-02T00:00:00Z", member: "urn:ngsi-ld:Vehicle:1"}
                ]
            });
            assert!(evolution_refused(&payload), "{member}");
        }
    }

    /// The same seal forged on the single instance a clause 5.6.14
    /// modification carries.
    #[test]
    fn a_forged_seal_on_a_modified_instance_is_refused() {
        let payload = fragment(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Vehicle"}));
        assert!(refused(&payload, |decoded| admit_written_instance(&entity(), decoded, &instance())));
    }

    /// An ordinary Temporal Evolution passes, instances and all.
    #[test]
    fn a_temporal_evolution_of_ordinary_instances_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": 20, "observedAt": "2024-01-02T00:00:00Z"}
            ]
        });
        assert!(!evolution_refused(&payload));
        let modified = fragment(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"}));
        assert!(!refused(&modified, |decoded| admit_written_instance(&entity(), decoded, &instance())));
    }

    /// Clause 5.6.14.3 takes an `EntityTemporal` Fragment; the bare instance it
    /// would contain is refused before anything is forwarded or stored.
    #[test]
    fn a_bare_modified_instance_is_refused() {
        let bare = json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"});
        assert!(refused(&bare, |decoded| admit_written_instance(&entity(), decoded, &instance())));
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

    fn decoded(payload: &Value) -> WrittenTemporalEntity {
        WrittenTemporalEntity::from_request_text(payload.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a decodable Temporal Evolution")
    }

    /// Clause 5.4: a Fragment's `id` "shall be equal to the id of the target
    /// (mutated) NGSI-LD element"; one naming another Entity is data clause
    /// 5.5.4 answers with `BadRequestData`, before anything is forwarded.
    #[test]
    fn added_attributes_naming_another_entity_are_refused() {
        let payload = decoded(&added_attributes(Some("urn:ngsi-ld:Vehicle:2")));
        let refusal = admit_added_attributes(&entity(), &payload, &instance()).expect_err("a Fragment naming another Entity is refused");
        pretty_assertions::assert_eq!(refusal.problem().error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(refusal.problem().status_code(), StatusCode::BAD_REQUEST);
    }

    /// Clause 5.4: a Fragment naming its target, or naming none where the
    /// request path does, is admitted.
    #[test]
    fn added_attributes_naming_the_target_or_no_id_are_admitted() {
        for id in [Some("urn:ngsi-ld:Vehicle:1"), None] {
            assert!(
                admit_added_attributes(&entity(), &decoded(&added_attributes(id)), &instance()).is_ok(),
                "{id:?}"
            );
        }
    }

    /// Clause 5.5.4's second bullet excepts "the representation of deleted
    /// Properties, Relationships or Language Properties in notifications and
    /// the temporal evolution", so an instance recording a deletion is data a
    /// Temporal Evolution holds rather than a payload to refuse.
    #[test]
    fn an_instance_recording_a_deletion_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2024-01-02T00:00:00Z"}]
        });
        assert!(!evolution_refused(&payload));
    }

    /// Clause 5.2.4 fixes the attribute types, so a token outside the eight is
    /// not an attribute wherever it is written.
    #[test]
    fn an_instance_declaring_a_token_outside_clause_5_2_4_is_refused() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "NotAnAttributeType", "value": 10}]
        });
        assert!(evolution_refused(&payload));
    }

    /// Clause 4.6.4: a key JSON-LD expansion would drop cannot be returned as
    /// written, in a temporal instance as anywhere else.
    #[test]
    fn a_keyword_form_key_in_a_temporal_instance_is_refused() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 10, "observedAt": "2024-01-01T00:00:00Z", "@foo": 1}]
        });
        assert!(evolution_refused(&payload));

        let modified = fragment(&json!({"type": "Property", "value": 10, "unit": {"@foo": {}}}));
        assert!(refused(&modified, |decoded| admit_written_instance(&entity(), decoded, &instance())));
    }

    /// The `json` element of a `JsonProperty` holds native JSON, so a
    /// keyword-form key inside one is data (clause 5.5.7, closing note).
    #[test]
    fn a_keyword_form_key_inside_a_temporal_json_property_is_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "config": [{"type": "JsonProperty", "json": {"@foo": "data"}, "observedAt": "2024-01-01T00:00:00Z"}]
        });
        assert!(!evolution_refused(&payload));
    }
}
