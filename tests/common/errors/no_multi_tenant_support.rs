//! `NoMultiTenantSupport` (501) Problem-Details tests (NGSI-LD 5.5.2).
//!
//! The broker returns this error from `src/tenancy/runtime.rs:117` when
//! `tenancy.enabled = false` and an incoming request carries an
//! `NGSILD-Tenant` header naming a tenant. Exercising it requires a
//! broker build / config with multi-tenancy disabled, which the shared
//! test fixture runs with multi-tenancy **enabled**.
//!
//! Scaffold documents the gap; once the test harness can spin up a
//! second broker instance with `tenancy.enabled = false` (or the broker
//! gains a per-request toggle), drop `#[ignore]` and populate.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "Shared broker is started with multi-tenancy enabled; follow-up: run a second broker instance with tenancy.enabled=false or expose a runtime toggle."]
async fn no_multi_tenant_support_documents_gap_until_disabled_broker_available() {}
