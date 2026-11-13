//! A location instance replaced by a property is refused and kept.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, instance_id_at},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ModifyAttributeInstance},
    model::{
        Attribute,
        AttributeName,
        ErrorType,
        GeoProperty,
        Lossless,
        NgsiLdMediaType,
        RawJson,
        Value,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
};

/// Pins: Modify Attribute instance of a `location` instance with a Fragment holding it as a Property answers 400
/// `BadRequestData` naming `location` and leaves the instance at the position it held
/// Clause: 5.2.20, 5.2.4, 5.5.4, 5.6.14.4
#[gtest]
#[tokio::test]
async fn n_tmp_mod_9006_a_location_instance_replaced_by_a_property_is_refused_and_kept() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_id = namespace.urn("GeoMemberProbe", "temporal")?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    let probe = TemporalEntity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(14.5, 46.05)?)).observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&probe).send().await?.created()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let location_name: AttributeName = "location".parse()?;
    let instance_id = instance_id_at(json.temporal().retrieve(&probe_id).send().await?.body(), &location_name, 0)?;
    let fragment = TemporalEntity::builder().id(probe_id.clone()).entity_type(probe_type).build()?;
    // The model admits `location` only as a GeoProperty, so the replacement instance is written as text.
    let location: RawJson = r#"[{"type": "Property", "value": "somewhere", "observedAt": "2025-01-01T00:00:00Z"}]"#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("location", &location);
    let placeholder = Attribute::from(GeoProperty::builder(Shape::Point(Position::try_new(2.35, 48.85)?)).build()?);
    let request = broker
        .unchecked(
            &ModifyAttributeInstance::builder()
                .id(&probe_id)
                .name(&location_name)
                .instance_id(&instance_id)
                .instance(&placeholder)
                .build(),
        )?
        .document(&document);
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'location' must be a GeoProperty")
    );
    let evolution = json.temporal().retrieve(&probe_id).send().await?;
    expect_that!(
        evolution.body(),
        has_evolution(
            "location",
            nth(0, any_attribute().value_member(["coordinates", "0"], Value::Number("14.5".parse()?)))
        )
    );
    Ok(())
}
