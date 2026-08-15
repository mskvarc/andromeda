//! Projection of policy batch outcomes into clause 5.2.16 wire values.

use crate::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::batch_operation::{BatchEntityError, BatchOperationResult};

/// Project one policy error through its binding-supplied exhaustive classifier.
pub fn batch_entity_error<E>(entity_id: Uri, error: &E, classify: impl FnOnce(&E) -> ClassifiedReport) -> BatchEntityError {
    let report = classify(error);
    BatchEntityError::new(entity_id, report.error_type(), report.detail())
}

/// Project a complete policy batch outcome into the clause 5.2.16 wire envelope.
pub fn batch_operation_result<E>(success: Vec<Uri>, errors: Vec<(Uri, E)>, classify: impl Fn(&E) -> ClassifiedReport) -> BatchOperationResult {
    let errors = errors
        .into_iter()
        .map(|(entity_id, error)| batch_entity_error(entity_id, &error, &classify))
        .collect();
    BatchOperationResult { success, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::error::Error;

    #[test]
    fn projection_preserves_successes_failures_and_supplied_classification() -> Result<(), Box<dyn Error>> {
        let success = "urn:ngsi-ld:Entity:1".parse::<Uri>()?;
        let failed = "urn:ngsi-ld:Entity:2".parse::<Uri>()?;
        let result = batch_operation_result(vec![success], vec![(failed, "invalid")], |detail| {
            ClassifiedReport::public(NgsiLdErrorType::BadRequestData, *detail)
        });
        pretty_assertions::assert_eq!(result.success[0].as_str(), "urn:ngsi-ld:Entity:1");
        pretty_assertions::assert_eq!(result.errors[0].entity_id.as_str(), "urn:ngsi-ld:Entity:2");
        pretty_assertions::assert_eq!(result.errors[0].error.detail, "invalid");
        Ok(())
    }
}
