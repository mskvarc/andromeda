//! The refusals that fail one batch item while the rest of the batch goes on.
//!
//! Clause 5.6.8.4: "If while processing an Entity there is any kind of error or
//! abnormal situation, a `BatchEntityError` shall be added to E". Every refusal
//! here is about the content of one item whose `id` is a URI, so it is
//! reported against that `id` and the rest of the batch is applied.

use crate::undecodable_body::UndecodableBody;
use andromeda_ngsi_ld_lexicon::{identifier_length::IdentifierTooLong, uri::Uri};
use andromeda_ngsi_ld_number::json_value::text_admission::JsonTextRefusal;
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use thiserror::Error;

/// Why one batch item is refused while the rest of the batch goes on.
#[derive(Debug, Error)]
pub enum BatchItemRefusal {
    /// The `id` is a URI, but longer than an identifier may be.
    #[error("the 'id' is not storable: {0}")]
    IdTooLong(#[from] IdentifierTooLong),
    /// Clause 6.3.5: an `application/json` item carries an `@context`.
    #[error("an application/json batch item must not contain @context")]
    ContextInJson,
    /// Clause 6.3.5: an `application/ld+json` item carries no `@context`.
    #[error("an application/ld+json batch item must contain @context")]
    NoContextInLdJson,
    /// A number the model cannot represent, a string or member name the broker
    /// cannot hold, a member named like the reader's own number tokens, or an
    /// object naming one member twice.
    #[error(transparent)]
    JsonText(#[from] JsonTextRefusal),
    /// A member that does not decode, named by its path.
    #[error(transparent)]
    Undecodable(#[from] UndecodableBody),
}

impl BatchItemRefusal {
    /// The clause 5.2.17 error this refusal reports against `entity_id`.
    ///
    /// Every refusal is content that is not syntactically correct according to
    /// the API data type definitions, which clause 5.5.4 makes
    /// `BadRequestData`.
    #[must_use]
    pub fn against(self, entity_id: Uri) -> BatchEntityError {
        BatchEntityError::new(entity_id, NgsiLdErrorType::BadRequestData, &self.to_string())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::batch_item_refusal::BatchItemRefusal;
    use andromeda_ngsi_ld_lexicon::{identifier_length::IdentifierTooLong, uri::Uri};
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};

    #[test]
    fn an_over_long_id_names_the_member_and_the_bound() {
        pretty_assertions::assert_eq!(
            BatchItemRefusal::IdTooLong(IdentifierTooLong { length: 1025 }).to_string(),
            "the 'id' is not storable: an identifier of 1025 bytes exceeds the 1024 bytes an identifier may occupy"
        );
    }

    #[test]
    fn a_refusal_is_reported_as_bad_request_data_against_the_items_id() {
        let entity_id: Uri = "urn:ngsi-ld:Vehicle:1".parse().expect("a URI");
        let error = BatchItemRefusal::ContextInJson.against(entity_id);
        pretty_assertions::assert_eq!(error.entity_id.as_str(), "urn:ngsi-ld:Vehicle:1");
        pretty_assertions::assert_eq!(
            error.error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        pretty_assertions::assert_eq!(error.error.detail, "an application/json batch item must not contain @context");
        assert!(error.registration_id.is_none());
    }
}
