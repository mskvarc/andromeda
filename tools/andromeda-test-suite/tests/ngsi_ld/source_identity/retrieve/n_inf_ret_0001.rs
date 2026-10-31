//! Answers the context source identity.

use andromeda_test_suite::broker;
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Retrieve Context Source Identity Information without an Accept header answers 200 with a
/// `ContextSourceIdentity` holding a URI id and every mandatory member
/// Clause: 5.15.1.4, 5.2.40, 6.33.3.1
#[gtest]
#[tokio::test]
async fn n_inf_ret_0001_answers_the_context_source_identity() -> Result<()> {
    let broker = broker::front()?;
    broker.with_accept(Accept::any()).source_identity().send().await?;
    Ok(())
}
