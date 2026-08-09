//! Transitional projection of snapshot purge failures into batch wire values.

use crate::{error::AndromedaError, http::error::category};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_problem_details::batch_operation::BatchEntityError;

/// Report a broker failure against one snapshot of a purge batch.
///
/// An internal failure is logged in full and reported as the bare phrase
/// "Internal error", for the same reason a single-response failure is: the
/// message may name a table, a query or a host.
pub fn snapshot_error(snapshot_id: Uri, error: &AndromedaError) -> BatchEntityError {
    let error_type = category::of(error.kind());

    if error.is_internal() {
        tracing::error!(
            error = %error,
            kind = ?error.kind(),
            snapshot_id = %snapshot_id,
            "Internal batch error (sanitized before sending to client)"
        );
        return BatchEntityError::new(snapshot_id, error_type, "Internal error");
    }

    BatchEntityError::new(snapshot_id, error_type, &error.to_string())
}
