//! The deployment names its brokers and reports its extras.

use andromeda_test_suite::{body_json::json_equals, broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information answers the configured identity of each broker of the multi
/// stack and a `contextSourceExtras` object that reports deployment multi verbatim
/// Clause: 5.2.40, 5.15.1.4
#[gtest]
#[tokio::test]
async fn a_inf_ret_7003_the_deployment_names_its_brokers_and_reports_its_extras() -> Result<()> {
    for (role, name) in [(BrokerRole::Front, "a"), (BrokerRole::B, "b"), (BrokerRole::C, "c"), (BrokerRole::D, "d")] {
        let broker = broker::client(role)?;
        let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
        expect_that!(identity.id.to_string(), ends_with(format!(":broker-{name}")));
        expect_that!(identity.alias.to_string(), eq(&format!("broker-{name}")));
        // The extras are raw JSON in the model, so the deployment member is read from their text.
        let extras = identity.extras.as_ref().map(ToString::to_string).or_fail()?;
        expect_that!(extras.as_bytes(), json_equals("$.deployment", "multi"));
    }
    Ok(())
}
