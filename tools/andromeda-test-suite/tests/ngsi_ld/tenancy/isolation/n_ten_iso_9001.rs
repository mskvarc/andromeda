//! Tenants whose names fold to one label hold their own entities.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// The Tenant names the test creates: two that differ only in letter case, and two long ones that differ only after
/// their first 40 characters.
const TENANT_NAMES: [&str; 4] = [
    "TEnAnTLABelProbEcase",
    "TenANTlaBELPRobEcase",
    "tenantlabelprobetruncatedcommonprefixabc-2o6a",
    "tenantlabelprobetruncatedcommonprefixabc-utob",
];

/// Pins: two Tenants whose names differ only in letter case, and two whose names differ only after their first 40
/// characters, are four Tenants: an entity created with the same id in each is created in each, and reads back from
/// each with that Tenant's own name as its value
/// Clause: 4.14, 5.5.10, 6.3.14
/// Tenant: private (the subject is Tenants whose names lowercased and shortened are one name; the names are fixed, and
/// each run creates an entity of its own in them)
#[gtest]
#[tokio::test]
async fn n_ten_iso_9001_tenants_whose_names_fold_to_one_label_hold_their_own_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let mut tenants = Vec::new();
    for tenant_name in TENANT_NAMES {
        let tenant = broker::provisioned_tenant(&broker, tenant_name.parse()?, &namespace).await?;
        let vehicle = Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("name", tenant_name)
            .build()?;
        tenant.entities().create(&vehicle).send().await?.done()?;
        tenants.push((tenant_name, tenant));
    }
    for (tenant_name, tenant) in &tenants {
        let retrieved = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&vehicle_id)
            .send()
            .await?;
        expect_that!(retrieved.body(), has_attribute("name", any_attribute().value(*tenant_name)));
    }
    Ok(())
}
