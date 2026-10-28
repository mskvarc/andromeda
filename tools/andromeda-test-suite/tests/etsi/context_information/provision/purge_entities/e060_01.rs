//! ETSI `060_01`: a Purge Entities without any selector is refused with 400 `BadRequestData` (clauses 5.6.21 and 6.4.3.3).

use crate::suite_keywords::refused;
use andromeda_test_suite::{broker, namespace::Namespace, wire::request_to};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::ErrorType;

/// Pins: Check that a `BadRequestData` error is returned when no query parameter is provided
/// Clause: 5.6.21, 6.4.3.3
/// Tenant: private (the purge names no selector, so a broker that failed to refuse it would delete every entity of the
/// shared tenant)
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e060_01_01_purge_entities_without_any_query_parameter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    refused(&tenant, request_to(&tenant, Method::DELETE, "entities/")?, ErrorType::BadRequestData).await
}
