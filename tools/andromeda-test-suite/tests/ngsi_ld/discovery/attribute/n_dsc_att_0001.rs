//! A tenant holding no entity lists nothing.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Retrieve Available Attributes in a tenant where no entity exists answers an `AttributeList` whose
/// `attributeList` is empty
/// Clause: 5.7.8.4, 5.2.27, 6.27.3.1
/// Tenant: private (the subject is the completeness of the unfiltered attribute list, empty only in a tenant no
/// other file writes to)
#[gtest]
#[tokio::test]
async fn n_dsc_att_0001_a_tenant_holding_no_entity_lists_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let attributes = tenant.with_accept(Accept::any()).attributes().list().send().await?;
    expect_that!(attributes.attribute_list, len(eq(0)));
    Ok(())
}
