//! Recording a request the tenant runtime cache shed as saturated.
//!
//! A 503 tells the client only to retry. The operator learns from the log line
//! and the counter which wait shed the request and how long it had waited,
//! which is what tells a slow host apart from a pool that is too small.

use crate::error::TenancyError;
use andromeda_telemetry::tenancy::record_shed_request;
use std::time::Instant;
use tracing::warn;

/// Await one request's tenancy work and, when it is shed as saturated, log the
/// tenant, the wait and the seconds since `work` started, and count the shed
/// request by its wait.
///
/// The line is emitted on the caller's span, which is the request's span on
/// every HTTP path.
pub(super) async fn report_shed<T>(work: impl Future<Output = Result<T, TenancyError>>) -> Result<T, TenancyError> {
    let started = Instant::now();
    let result = work.await;
    if let Err(TenancyError::Saturated { tenant, wait }) = &result {
        warn!(
            %tenant,
            %wait,
            waited_seconds = started.elapsed().as_secs_f64(),
            "the request was shed as saturated"
        );
        record_shed_request((*wait).into());
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::{error::TenancyError, runtime::shed_report::report_shed, saturated_wait::SaturatedWait, tenant_ref::TenantRef};
    use andromeda_telemetry::{error::TelemetryError, names::TENANT_REQUESTS_SHED_TOTAL, recorder::TelemetryRecorder};
    use std::{
        future::{Future, ready},
        pin::pin,
        task::{Context, Poll, Waker},
    };

    /// Drive a future that never suspends to its output.
    fn resolved<T>(work: impl Future<Output = T>) -> Option<T> {
        match pin!(work).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(output) => Some(output),
            Poll::Pending => None,
        }
    }

    #[test]
    fn a_shed_request_is_counted_under_its_wait_and_others_are_not() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            for wait in [
                SaturatedWait::RegistrationTransaction,
                SaturatedWait::RegistrationTransaction,
                SaturatedWait::BuildLock,
            ] {
                let shed = resolved(report_shed(ready(Err::<(), TenancyError>(TenancyError::Saturated {
                    tenant: TenantRef::UnnamedDefault,
                    wait,
                }))));
                assert!(
                    matches!(shed, Some(Err(TenancyError::Saturated { wait: reported, .. })) if reported == wait),
                    "{shed:?}"
                );
            }
            let refused = resolved(report_shed(ready(Err::<(), TenancyError>(TenancyError::MultiTenancyDisabled))));
            assert!(matches!(refused, Some(Err(TenancyError::MultiTenancyDisabled))), "{refused:?}");
            assert!(matches!(resolved(report_shed(ready(Ok::<(), TenancyError>(())))), Some(Ok(()))));
        });
        let rendered = recorder.render();
        assert!(
            rendered.contains(&format!("{TENANT_REQUESTS_SHED_TOTAL}{{wait=\"registration-transaction\"}} 2")),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("{TENANT_REQUESTS_SHED_TOTAL}{{wait=\"build-lock\"}} 1")),
            "{rendered}"
        );
        assert!(!rendered.contains("multi-tenancy"), "{rendered}");
        Ok(())
    }
}
