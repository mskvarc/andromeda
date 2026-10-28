//! ETSI `035_03`: a context source registration whose id is not known is not deleted (clause 5.9.4).

use crate::suite_keywords::not_found;
use andromeda_test_suite::{broker, namespace::Namespace, wire::request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::registration::RegistrationId;

/// Pins: Check that one cannot delete a context source registration by id if the id is not known to the system
/// Clause: 5.9.4
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e035_03_01_delete_a_context_source_registration_by_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    not_found(&broker, request_to(&broker, Method::DELETE, &format!("csourceRegistrations/{id}"))?).await
}
