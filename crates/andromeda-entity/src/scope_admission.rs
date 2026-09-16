//! Whether a write may set, replace or delete the entity `scope` member.
//!
//! Clause 4.18 gives the member one grammar with two readings: the scope paths
//! an entity belongs to, and the NGSI-LD Null, of which the clause says the
//! special string "shall be only used and only appear in case of deleted
//! scopes". Which of the two a request may state is the clause 5.5.4 rule every
//! other first-level member is bound by — "Any attempt to use
//! `urn:ngsi-ld:null` as a first level member value, with the exception of
//! NGSI-LD Fragments used in partial update and merge operations [...] shall
//! result in an error of type BadRequestData" — so `scope` takes the same
//! [`NullSentinelAdmission`] the attribute members take.
//!
//! The attribute walk of [`crate::null_sentinel_admission`] steps over `scope`,
//! because clause 4.18 makes it a non-reified Entity member rather than an
//! attribute: it has no family, no `value` and no `datasetId`, so none of the
//! prohibitions that walk applies is about it. This module states the one that
//! is, and validates in the same pass that each path the member does carry is a
//! path the broker can store, so a level no label-path index could hold is bad
//! input rather than a failed database cast.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, null_sentinel_admission::NullSentinelAdmission};
use andromeda_ngsi_ld_model::expanded::scope_member::{ScopeMember, read_scope_member};
use serde_json::Value;

/// Validate the `scope` member of an expanded entity payload or fragment.
///
/// # Errors
///
/// Reports [`EntityError::Validation`], which the HTTP binding answers with the
/// `BadRequestData` of clause 5.5.4, when the member is not a value clause 4.18
/// admits, and when it is the NGSI-LD Null under an `admission` that refuses a
/// deletion sentinel.
pub(crate) fn validate_scope_member(expanded: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    let member = read_scope_member(expanded).map_err(|error| EntityError::Validation(EntityValidationReason::Scope(error)))?;
    match (member, admission) {
        (Some(ScopeMember::Deleted), NullSentinelAdmission::Refused) => Err(EntityError::Validation(EntityValidationReason::ScopeNull)),
        (Some(ScopeMember::Deleted), NullSentinelAdmission::PartialUpdateFragment | NullSentinelAdmission::MergeFragment)
        | (
            Some(ScopeMember::Paths(_)) | None,
            NullSentinelAdmission::Refused | NullSentinelAdmission::PartialUpdateFragment | NullSentinelAdmission::MergeFragment,
        ) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{null_sentinel_admission::NullSentinelAdmission, scope_admission::validate_scope_member};
    use serde_json::{Value, json};

    /// Every admission, so a case can state what each one does with a payload.
    const ADMISSIONS: [NullSentinelAdmission; 3] = [
        NullSentinelAdmission::Refused,
        NullSentinelAdmission::PartialUpdateFragment,
        NullSentinelAdmission::MergeFragment,
    ];

    /// One expanded entity whose `scope` member holds the given values.
    fn entity_scoped(values: &[&str]) -> Value {
        let member: Vec<Value> = values.iter().map(|raw| json!({"@value": raw})).collect();
        json!([{"@id": "urn:ngsi-ld:Vehicle:1", "https://uri.etsi.org/ngsi-ld/scope": member}])
    }

    #[test]
    fn a_hierarchical_scope_is_admitted_under_every_admission() {
        for admission in ADMISSIONS {
            validate_scope_member(&entity_scoped(&["/Madrid/Gardens"]), admission).expect("clause 4.18 admits a hierarchical path");
        }
    }

    #[test]
    fn a_payload_without_a_scope_member_is_admitted_under_every_admission() {
        for admission in ADMISSIONS {
            validate_scope_member(&json!([{"@id": "urn:ngsi-ld:Vehicle:1"}]), admission).expect("clause 4.18 makes scope optional");
        }
    }

    /// Clause 5.5.4: a first level member set to the NGSI-LD Null is
    /// `BadRequestData` outside "NGSI-LD Fragments used in partial update and
    /// merge operations", and `scope` is a first level member.
    #[test]
    fn the_null_scope_is_refused_outside_a_fragment_and_admitted_inside_one() {
        let error = validate_scope_member(&entity_scoped(&["urn:ngsi-ld:null"]), NullSentinelAdmission::Refused)
            .expect_err("clause 5.5.4 forbids a first level null here");
        assert!(error.to_string().contains("NGSI-LD Null"), "{error}");
        for admission in [NullSentinelAdmission::PartialUpdateFragment, NullSentinelAdmission::MergeFragment] {
            validate_scope_member(&entity_scoped(&["urn:ngsi-ld:null"]), admission).expect("a fragment deletes the scope with the sentinel");
        }
    }

    /// Clause 4.18 admits the sentinel only "in case of deleted scopes", so a
    /// request cannot delete the scope and set it in the same member — under
    /// any admission.
    #[test]
    fn a_null_beside_a_path_is_refused_under_every_admission() {
        for admission in ADMISSIONS {
            let error =
                validate_scope_member(&entity_scoped(&["/Madrid", "urn:ngsi-ld:null"]), admission).expect_err("the sentinel cannot stand beside a path");
            assert!(error.to_string().contains("deleted scope"), "{admission:?}: {error}");
        }
    }

    /// A scope level beyond what a label-path index holds is refused here, so
    /// the request is answered as bad input rather than as an internal failure
    /// raised by a database cast.
    #[test]
    fn a_scope_level_beyond_the_storable_bound_is_refused_under_every_admission() {
        let level = format!("/{}", "a".repeat(1001));
        for admission in ADMISSIONS {
            let error = validate_scope_member(&entity_scoped(&[&level]), admission).expect_err("the level cannot be stored");
            assert!(error.to_string().contains("scope level"), "{admission:?}: {error}");
        }
    }
}
