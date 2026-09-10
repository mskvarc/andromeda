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

use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_representation::attribute_type_validation::{validate_attribute_node, validate_fragment_attribute_types};
use axum::http::Uri as HttpUri;
use serde_json::Value;

/// Screen one written `EntityTemporal` payload (clauses 5.6.11 and 5.6.12)
/// before it is forwarded or stored.
///
/// # Errors
///
/// Answers `BadRequestData` for the first attribute node clauses 5.2.4 and
/// 4.5.2.2 do not admit.
pub(crate) fn admit_written_evolution(payload: &Value, instance: &HttpUri) -> Result<(), ProblemResponse> {
    validate_fragment_attribute_types(payload)
        .map_err(|error| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()).with_instance(instance.clone()))
    // clone: the problem details own the instance the borrowed request names
}

/// Screen the one attribute instance a clause 5.6.14 modification carries.
///
/// # Errors
///
/// Answers `BadRequestData` on the same terms as [`admit_written_evolution`].
pub(crate) fn admit_written_instance(attribute: &AttributeName, payload: &Value, instance: &HttpUri) -> Result<(), ProblemResponse> {
    validate_attribute_node(attribute.as_str(), payload)
        .map_err(|error| ClassifiedReport::public(NgsiLdErrorType::BadRequestData, error.to_string()).with_instance(instance.clone()))
    // clone: the problem details own the instance the borrowed request names
}

#[cfg(test)]
mod tests {
    use crate::written_evolution_admission::{admit_written_evolution, admit_written_instance};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use axum::http::Uri as HttpUri;
    use serde_json::json;

    fn instance() -> HttpUri {
        "/ngsi-ld/v1/temporal/entities".parse().expect("a valid request target")
    }

    fn attribute() -> AttributeName {
        "belongsTo".parse().expect("a valid attribute name")
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
            assert!(admit_written_evolution(&payload, &instance()).is_err(), "{member}");
        }
    }

    /// The same seal forged on the single instance a clause 5.6.14
    /// modification carries.
    #[test]
    fn a_forged_seal_on_a_modified_instance_is_refused() {
        let payload = json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "entityTypeSealed": "Vehicle"});
        assert!(admit_written_instance(&attribute(), &payload, &instance()).is_err());
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
        assert!(admit_written_evolution(&payload, &instance()).is_ok());
        assert!(admit_written_instance(&attribute(), &json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"}), &instance()).is_ok());
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
        assert!(admit_written_evolution(&payload, &instance()).is_ok());
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
        assert!(admit_written_evolution(&payload, &instance()).is_err());
    }
}
