//! An unequal term and a value list read the operations array by membership.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q::Query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with a csf over `operations` answers 200 with the registrations
/// whose `operations` includes, or does not include, the value or one of the listed values
/// Clause: 4.9, 5.2.9, 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9400_an_unequal_term_and_a_value_list_read_the_operations_array_by_membership() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let origin = SuiteConfig::get()?.unreachable_origin();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicles_id = namespace.registration_id("vehicles")?;
    let buildings_id = namespace.registration_id("buildings")?;
    // A registration of one type and no identifier is asked by every retrieval by identifier of the tenant, so one
    // left behind answers other tests' retrievals with a cooldown warning.
    let _registration_cleanup = Cleanup::of(&broker).registration(&vehicles_id).registration(&buildings_id);
    for (id, operations) in [
        (&vehicles_id, vec![DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity]),
        (&buildings_id, vec![DistributedOperation::RetrieveEntity]),
    ] {
        let registration = CSourceRegistration::builder(origin.as_str())
            .id(id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .operations(operations)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    // Clause 4.9: an Equal term holds when the target array includes the value, or any value of a list, and an
    // Unequal term when it includes none of them.
    for (filter, expected) in [
        (r#"operations!="queryEntity""#, vec![&buildings_id]),
        (r#"operations=="updateEntity","queryEntity""#, vec![&vehicles_id]),
        (r#"operations!="updateEntity","queryEntity""#, vec![&buildings_id]),
        (r#"operations=="retrieveEntity""#, vec![&vehicles_id, &buildings_id]),
        (r#"operations=="deleteEntity""#, vec![]),
        (r#"operations!="deleteEntity""#, vec![&vehicles_id, &buildings_id]),
    ] {
        let page = json
            .registrations()
            .query()
            .entity_type(&vehicle_type)
            .csf(filter.parse::<Query>()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(expected.len())), "csf {filter}");
        for id in expected {
            expect_that!(page.items(), contains(has_id(id)), "csf {filter}");
        }
    }
    Ok(())
}
