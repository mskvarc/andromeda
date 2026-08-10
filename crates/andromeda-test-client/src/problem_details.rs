//! Focused assertions for NGSI-LD Problem Details and entity identifiers.

use iri_rs::Iri;
use serde_json::Value;
use thiserror::Error;

/// Validate the required Problem Details fields and expected NGSI-LD error type.
pub fn validate_problem_details(body: &Value, expected_type: &Iri<&str>) -> Result<(), ProblemDetailsAssertionError> {
    let object = body.as_object().ok_or(ProblemDetailsAssertionError::NotObject)?;
    let actual_type = object
        .get("type")
        .ok_or(ProblemDetailsAssertionError::MissingType)?
        .as_str()
        .ok_or(ProblemDetailsAssertionError::TypeNotString)?;
    if actual_type != expected_type.as_str() {
        return Err(ProblemDetailsAssertionError::TypeMismatch {
            expected: expected_type.as_str().to_owned(),
            actual: actual_type.to_owned(),
        });
    }
    object
        .get("title")
        .ok_or(ProblemDetailsAssertionError::MissingTitle)?
        .as_str()
        .ok_or(ProblemDetailsAssertionError::TitleNotString)?;
    Ok(())
}

/// Validate an entity object's identifier.
pub fn validate_entity_id(body: &Value, expected_id: &Iri<&str>) -> Result<(), EntityAssertionError> {
    let actual = body
        .as_object()
        .ok_or(EntityAssertionError::NotObject)?
        .get("id")
        .ok_or(EntityAssertionError::MissingId)?
        .as_str()
        .ok_or(EntityAssertionError::IdNotString)?;
    if actual == expected_id.as_str() {
        Ok(())
    } else {
        Err(EntityAssertionError::IdMismatch {
            expected: expected_id.as_str().to_owned(),
            actual: actual.to_owned(),
        })
    }
}

/// Problem Details shape or value mismatch.
#[derive(Debug, Error)]
pub enum ProblemDetailsAssertionError {
    /// The body is not a JSON object.
    #[error("Problem Details body is not an object")]
    NotObject,
    /// The `type` field is absent.
    #[error("Problem Details body is missing `type`")]
    MissingType,
    /// The `type` field is not a string.
    #[error("Problem Details `type` is not a string")]
    TypeNotString,
    /// The error type differs from the expected IRI.
    #[error("Problem Details type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected error type.
        expected: String,
        /// Actual error type.
        actual: String,
    },
    /// The `title` field is absent.
    #[error("Problem Details body is missing `title`")]
    MissingTitle,
    /// The `title` field is not a string.
    #[error("Problem Details `title` is not a string")]
    TitleNotString,
}

/// Entity response shape or identifier mismatch.
#[derive(Debug, Error)]
pub enum EntityAssertionError {
    /// The body is not a JSON object.
    #[error("entity body is not an object")]
    NotObject,
    /// The entity `id` field is absent.
    #[error("entity body is missing `id`")]
    MissingId,
    /// The entity `id` field is not a string.
    #[error("entity `id` is not a string")]
    IdNotString,
    /// The entity identifier differs from the expected IRI.
    #[error("entity id mismatch: expected {expected}, got {actual}")]
    IdMismatch {
        /// Expected identifier.
        expected: String,
        /// Actual identifier.
        actual: String,
    },
}
