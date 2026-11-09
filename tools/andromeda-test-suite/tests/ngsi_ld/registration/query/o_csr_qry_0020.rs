//! The last pointer leads to the final page.

use andromeda_test_suite::{broker, config::SuiteConfig, cursor_walk, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryRegistrations},
    model::NgsiLdMediaType,
    query::type_selection,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 2 and an empty cursor over five members offers a last pointer
/// leading to a page of two members holding the greatest identifier, with no next pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_csr_qry_0020_the_last_pointer_leads_to_the_final_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_type = namespace.entity_type("Building")?;
    for suffix in ["0000", "0001", "0002", "0003", "0004"] {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(namespace.registration_id(suffix)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
                .property_names(["name"])
                .build()?])
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let selection = type_selection::entity_type(&building_type).build()?;
    let query = QueryRegistrations::builder().entity_type(&selection).limit(2).build();
    let page = cursor_walk::first_page(&json, &query).await?.page;
    let last = json.send(&page.last_page().or_fail()?).await?;
    expect_that!(last.items(), len(eq(2)));
    expect_that!(last.items(), contains(has_id(&namespace.registration_id("0004")?)));
    expect_that!(last.next(), none());
    Ok(())
}
