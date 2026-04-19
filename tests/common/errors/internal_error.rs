//! `InternalError` (500) Problem-Details tests (NGSI-LD 5.5.2).
//!
//! `ErrorKind::Internal` is produced whenever a component that should be
//! infallible fails — typically SQLX / tenancy-provisioning errors or
//! malformed server-side state. There is no deterministic way to provoke
//! this class of failure from an HTTP client against a healthy broker; any
//! scenario that does (database outage, corrupted schema) requires external
//! setup outside the test process.
//!
//! Scaffold left in place so a future operator who has a reliable trigger
//! (e.g. a fault-injection adapter) can flip this test on without
//! re-authoring the boilerplate.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "No deterministic trigger for InternalError from HTTP; follow-up: introduce fault-injection adapter to force a repository failure on demand."]
async fn internal_error_documents_gap_until_fault_injection_available() {}
