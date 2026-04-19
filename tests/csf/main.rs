//! Context-source filter (CSF) integration tests.
//!
//! The CSF filter applies at three surfaces:
//!
//! - Query CSRs: `GET /csourceRegistrations?csf=<expression>`
//! - Subscription matching: filter which registrations a
//!   `NotificationMatch` evaluation considers.
//! - Entity query forwarding: restrict which registered Context Sources a
//!   `/entities` GET is fanned out to.
//!
//! Each surface needs a multi-registration fixture with distinguishing
//! attributes and a companion `ContextSourceMock` to exercise the
//! forwarding path. The scaffold is in place; authoring is deferred.
//!
//! Naming convention: `csf_<surface>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "CSF coverage needs multi-registration fixtures + ContextSourceMock wiring across three surfaces (query CSRs, subscription matching, entity query forwarding); follow-up: author once fixture DSL lands."]
async fn csf_follow_up_requires_multi_surface_fixture() {}
