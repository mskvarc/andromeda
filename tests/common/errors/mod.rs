//! Problem-Details tests for every NGSI-LD error type (spec 5.5.2).
//!
//! Each file targets a single error URI. Tests that cannot deterministically
//! trigger their error without broker-side config changes or source-code
//! changes are marked `#[ignore]` with a follow-up note; the scaffolding
//! exists so future work can flip them on without re-authoring.

mod internal_error;
mod no_multi_tenant_support;
mod operation_not_supported;
mod too_complex_query;
mod too_many_results;
