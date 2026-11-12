//! An attrs selection pages the holders of every type once in identifier order.

use andromeda_test_suite::{
    broker,
    cursor_walk,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{NormalizedTemporal, QueryTemporalEvolution},
    },
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{TemporalAttributeName, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with attrs=load and no type, limit 2 and an empty cursor, over Evolutions
/// of the types Truck, Bus and Car, answers by cursor every Evolution holding load once in ascending identifier order:
/// one recorded under both Truck and Bus and one holding two dataset instances of load appear once, and a Truck
/// without load is left out
/// Clause: 5.5.9.1, 5.5.9.2, 5.7.4.4, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_9079_an_attrs_selection_pages_the_holders_of_every_type_once_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vehicle_0006_id = namespace.urn("Vehicle", "0006")?;
    let bus_type = namespace.entity_type("Bus")?;
    let vehicle_0002_id = namespace.urn("Vehicle", "0002")?;
    let vehicle_0005_id = namespace.urn("Vehicle", "0005")?;
    let car_type = namespace.entity_type("Car")?;
    let vehicle_0003_id = namespace.urn("Vehicle", "0003")?;
    let vehicle_0004_id = namespace.urn("Vehicle", "0004")?;
    let truck_type = namespace.entity_type("Truck")?;
    let vehicle_0001_id = namespace.urn("Vehicle", "0001")?;
    let vehicle_0006_evolution = TemporalEntity::builder()
        .id(vehicle_0006_id.clone())
        .entity_type(bus_type.clone())
        .property("load", Property::builder(6).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property(
            "load",
            Property::builder(60)
                .dataset_id(namespace.dataset_id("Dataset", "second")?)
                .observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0006_evolution).send().await?.created()?;
    let vehicle_0002_evolution = TemporalEntity::builder()
        .id(vehicle_0002_id.clone())
        .entity_type(bus_type.clone())
        .property("load", Property::builder(2).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0002_evolution).send().await?.created()?;
    let vehicle_0005_evolution = TemporalEntity::builder()
        .id(vehicle_0005_id.clone())
        .entity_type(car_type)
        .property("load", Property::builder(5).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0005_evolution).send().await?.created()?;
    let vehicle_0003_evolution = TemporalEntity::builder()
        .id(vehicle_0003_id.clone())
        .entity_type(vec![namespace.entity_type("Truck")?, bus_type.clone()])
        .property("load", Property::builder(3).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0003_evolution).send().await?.created()?;
    let vehicle_0004_evolution = TemporalEntity::builder()
        .id(vehicle_0004_id)
        .entity_type(truck_type.clone())
        .property("colour", Property::builder("red").observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0004_evolution).send().await?.created()?;
    let vehicle_0001_evolution = TemporalEntity::builder()
        .id(vehicle_0001_id.clone())
        .entity_type(truck_type.clone())
        .property("load", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    tenant.temporal().create_or_update(&vehicle_0001_evolution).send().await?.created()?;
    let window = temporal_query::after("1970-01-01T00:00:00Z".parse()?).build()?;
    let attrs: [TemporalAttributeName; 1] = ["load".parse()?];
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .attrs(&attrs)
        .limit(2)
        .format(NormalizedTemporal)
        .build();
    let page = cursor_walk::first_page(&tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), &query)
        .await?
        .page;
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_0001_id)));
    expect_that!(page.items(), nth(1, has_id(&vehicle_0002_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_0003_id)));
    expect_that!(page_2.items(), nth(1, has_id(&vehicle_0005_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.next(), none());
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, all!(has_id(&vehicle_0006_id), has_evolution("load", len(eq(2))))));
    Ok(())
}
