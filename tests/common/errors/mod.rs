//! Problem-Details tests for every NGSI-LD error type (spec 5.5.2).
//!
//! Each file targets a single error URI. Tests that cannot deterministically
//! trigger their error without broker-side config changes or source-code
//! changes are marked `#[ignore]` with a follow-up note; the scaffolding
//! exists so future work can flip them on without re-authoring.
//!
//! `OperationNotSupported` has no file: no handler emits it, so no request can reach the error. The framework-independent category remains covered by `andromeda-ngsi-ld-problem-details`, while HTTP encoding is covered by `andromeda-http-response`.

mod internal_error;
mod no_multi_tenant_support;
mod too_complex_query;
