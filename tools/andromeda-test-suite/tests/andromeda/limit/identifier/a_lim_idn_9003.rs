//! An expanded dataset id past the bound is refused and the default instance stays.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, DatasetId, Entity, EntityFragment, ErrorType, InlineContext, NgsiLdMediaType, Property},
};

/// Pins: Append Attributes with a temperature instance whose `datasetId` ds:x expands through an inline @context to
/// over 1,100 octets answers 400 `BadRequestData`, and the Entity still holds only its default temperature instance
/// 1
/// Clause: 4.5.5, 5.6.3, 5.5.4
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9003_an_expanded_dataset_id_past_the_bound_is_refused_and_the_default_instance_stays() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_long_expanded_dataset_id = namespace.urn("Building", "long-expanded-dataset")?;
    let building_type = namespace.entity_type("Building")?;
    let body_context = broker.with_context(Context::Multiple(vec![ContextEntry::Inline(r#"{"ds": "http://example.test/datasets/ipb2lhwfordul91dbqt0mlh7duvqlzt6jrrl1dv8c0q6s70zltxzbt987171ff2wg1zdm6pzag3mkb8ywlvwm2ce8imgdm0hj3ch57is44ewhqhbl6wevgk1rlcemwhn8q5elbrg4nzwh7lcpycnn5yd03qpohrekludb5krc5bran2o281iwvqaygl9xr6qmqr7kw655i8m5zvey7mkfdyvpd77jm9xa7ubh6degelq6tu079vudo6usappdhutbuhg2vlk4s0zwiztp8oxc3r68nfipt9i2dqpjh94uk6yevnyi318dz32xnsafhj3fn4ribkt8pmvz29lz291qnpo7rscp9pw3xhow0xiic96tgi6ozigx6qaak2vk4k6kgqxh4klf6q282uuoldhxea780v1go0qcv9rjuz6gbw8ek2a2y1exb4ynsh9kshc3muzl3pkebhmq05rtwrxtf94ojm1yjerxa3764mbnjtcb5h9wohow0kjqp41hwiqihspwwretz9omnijuqieod5shh0fab5307i6pluqcukfd82p966xoiruc6ebbt7yygw066gxh5wynpigsdyurxhrvxrerwus09lg5kplp1fx35272fw4h5trj828t2zz0p4x5h4tgxbwc4ryapzw3ndylnwnd7qdxfbf06ii6e6h8tczvmfcfm87l86ogksslr2vd90wyoswzrptodssejizw7t76f4i82nsck6bh79oqfbztr0gkr2t9evjqcytzcg75fvt7limn4c50g11ku6ho5zt82pt87o0k4h1zo54bq08gpfpu71zj18nv2ds6p4mvckkcoy696f7fyqb1w4xzsc06kl6q67uxi2ah2yr5gm756ndikkbqxbxbxhdp3nzejqgs7ag7eppz9elju4g8f8abb5tjairdwbqs23ccmb15uwp2dkitewk24756qvnokybcuroidmaz6tad5myhjqaq2x2ngbdu1rjpiek7h9wsvrj787fc9biv7kaf8ele655i8tr4pm5k5xhjbfshsbn9j2uk2j5e1vc4a9p702xdh0tz9iny2dabsyj9ebm06393oxl/"}"#.parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let building_long_expanded_dataset = Entity::builder()
        .id(building_long_expanded_dataset_id.clone())
        .entity_type(building_type)
        .property("temperature", 1)
        .build()?;
    broker.entities().create(&building_long_expanded_dataset).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .property("temperature", Property::builder(2).dataset_id("ds:x".parse::<DatasetId>()?))
        .build()?;
    let answer = body_context
        .with_responses()
        .entities()
        .append_attributes(&building_long_expanded_dataset_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_long_expanded_dataset_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("temperature", property().value(1).without(Member::DatasetId)));
    Ok(())
}
