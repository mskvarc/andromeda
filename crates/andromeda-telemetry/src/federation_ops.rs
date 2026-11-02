//! Operation accounting for outbound federation requests.

use crate::{
    names::{FEDERATION_REQUEST_DURATION_SECONDS, FEDERATION_REQUESTS_TOTAL, FEDERATION_UPSTREAM_STATUS_TOTAL, LABEL_STATUS},
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
    OperationTimer::start(FEDERATION_REQUESTS_TOTAL, FEDERATION_REQUEST_DURATION_SECONDS, operation.into())
}

/// The class of an HTTP status code (RFC 9110 section 15), as it appears in
/// the `status` label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr)]
pub enum StatusClass {
    /// 100 to 199.
    #[strum(serialize = "1xx")]
    Informational,
    /// 200 to 299.
    #[strum(serialize = "2xx")]
    Successful,
    /// 300 to 399.
    #[strum(serialize = "3xx")]
    Redirection,
    /// 400 to 499.
    #[strum(serialize = "4xx")]
    ClientError,
    /// 500 to 599.
    #[strum(serialize = "5xx")]
    ServerError,
    /// A code outside the classes RFC 9110 defines.
    #[strum(serialize = "other")]
    Unclassified,
}

impl StatusClass {
    /// The class `status` belongs to.
    #[must_use]
    pub const fn of(status: u16) -> StatusClass {
        match status {
            100..=199 => StatusClass::Informational,
            200..=299 => StatusClass::Successful,
            300..=399 => StatusClass::Redirection,
            400..=499 => StatusClass::ClientError,
            500..=599 => StatusClass::ServerError,
            _ => StatusClass::Unclassified,
        }
    }
}

/// Count one upstream response by the class of its HTTP status code.
pub fn record_upstream_status(status: u16) {
    let class: &'static str = StatusClass::of(status).into();
    counter!(FEDERATION_UPSTREAM_STATUS_TOTAL, LABEL_STATUS => class).increment(1);
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        federation_ops::{StatusClass, record_upstream_status},
        names::FEDERATION_UPSTREAM_STATUS_TOTAL,
        recorder::TelemetryRecorder,
    };

    #[test]
    fn upstream_statuses_are_counted_under_their_class() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            record_upstream_status(503);
            record_upstream_status(502);
            record_upstream_status(200);
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{FEDERATION_UPSTREAM_STATUS_TOTAL}{{status=\"5xx\"}} 2")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{FEDERATION_UPSTREAM_STATUS_TOTAL}{{status=\"2xx\"}} 1")),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_status_code_maps_to_its_class_at_every_boundary() {
        for (status, expected) in [
            (100, "1xx"),
            (199, "1xx"),
            (200, "2xx"),
            (299, "2xx"),
            (300, "3xx"),
            (399, "3xx"),
            (400, "4xx"),
            (499, "4xx"),
            (500, "5xx"),
            (599, "5xx"),
            (600, "other"),
            (99, "other"),
        ] {
            let class: &'static str = StatusClass::of(status).into();
            pretty_assertions::assert_eq!(class, expected, "{status}");
        }
    }
}
