//! An update off the axes is refused and leaves the value.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateAttributes},
    model::{
        Entity,
        EntityFragment,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: Update Attributes moving location to latitude -91 answers 400 `BadRequestData` naming the refused value,
/// and location keeps the position it held
/// Clause: 5.6.2.4, 4.7
#[gtest]
#[tokio::test]
async fn n_att_upd_9002_an_update_off_the_axes_is_refused_and_leaves_the_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let range_probe_updated_id = namespace.urn("RangeProbe", "updated")?;
    let range_probe_type = namespace.entity_type("RangeProbe")?;
    let range_probe_updated = Entity::builder()
        .id(range_probe_updated_id.clone())
        .entity_type(range_probe_type)
        .geo_property("location", Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&range_probe_updated).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (a position the constructor refuses: latitude -91 lies outside [-90, 90]), so `location` is written as text.
    let location: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, -91]}}"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("location", &location);
    let request = broker
        .unchecked(&UpdateAttributes::builder().id(&range_probe_updated_id).fragment(&fragment).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("-91")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&range_probe_updated_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .length_at(["coordinates"], 2)
                .value_member(["coordinates", "0"], Value::Number("14.5".parse()?))
                .value_member(["coordinates", "1"], Value::Number("46.05".parse()?))
        )
    );
    Ok(())
}
