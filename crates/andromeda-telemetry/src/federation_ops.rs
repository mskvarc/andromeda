//! Operation accounting for outbound federation requests.

use crate::{
    names::{FEDERATION_REQUEST_SECONDS, FEDERATION_REQUESTS_TOTAL, FEDERATION_UPSTREAM_STATUS_TOTAL, LABEL_STATUS},
    operation::OperationTimer,
};
use metrics::counter;
use strum::IntoStaticStr;

/// One outbound federation operation, as it appears in the `operation` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum FederationOperation {
    /// Forward a query to a context source (clause 5.7.2).
    Query,
    /// Forward a retrieval to a context source (clause 5.7.1).
    Retrieve,
    /// Forward a provisioning write to a context source (clause 5.7).
    Provision,
}

/// Start timing an outbound federation `operation`.
#[must_use]
pub fn start(operation: FederationOperation) -> OperationTimer {
    OperationTimer::start(FEDERATION_REQUESTS_TOTAL, FEDERATION_REQUEST_SECONDS, operation.into())
}

/// Count one upstream response by its HTTP status code.
pub fn record_upstream_status(status: u16) {
    counter!(FEDERATION_UPSTREAM_STATUS_TOTAL, LABEL_STATUS => status.to_string()).increment(1);
}

#[cfg(test)]
mod tests {
    use crate::{error::TelemetryError, federation_ops::record_upstream_status, names::FEDERATION_UPSTREAM_STATUS_TOTAL, recorder::TelemetryRecorder};

    #[test]
    fn an_upstream_status_is_counted_under_its_code() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| record_upstream_status(503));
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{FEDERATION_UPSTREAM_STATUS_TOTAL}{{status=\"503\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }
}
