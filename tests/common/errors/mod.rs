//! Problem-Details tests for every NGSI-LD error type (spec 5.5.2).
//!
//! Each file targets a single error URI. Tests that cannot deterministically
//! trigger their error without broker-side config changes or source-code
//! changes are marked `#[ignore]` with a follow-up note; the scaffolding
//! exists so future work can flip them on without re-authoring.
//!
//! `OperationNotSupported` has no file: its only emitter was the `/i18n/*`
//! handler, which no longer exists, so no request can reach the error. The
//! type itself is still mapped in `src/http/error/problem.rs` and covered by
//! that module's unit tests.

mod internal_error;
mod no_multi_tenant_support;
mod too_complex_query;
mod too_many_results;
