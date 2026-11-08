//! A tenant holding no entity lists nothing.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::http::headers::Accept;

/// Pins: Retrieve Available Entity Types in a tenant where no entity exists answers an `EntityTypeList` whose
/// `typeList` is empty
/// Clause: 5.7.5.4, 5.2.24, 6.25.3.1
/// Tenant: private (the subject is the completeness of the unfiltered type list, empty only in a tenant no other
/// file writes to)
#[gtest]
#[tokio::test]
async fn n_dsc_typ_0001_a_tenant_holding_no_entity_lists_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let types = tenant.with_accept(Accept::any()).entity_types().list().send().await?;
    expect_that!(types.type_list, len(eq(0)));
    Ok(())
}
