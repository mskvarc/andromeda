//! Scope is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceAttribute},
    model::{Attribute, AttributeName, Entity, ErrorType, NgsiLdMediaType, Property, Scope},
};

/// Pins: Replace Attribute addressed to scope is refused with 400 `BadRequestData` and the scope stays
/// Clause: 5.6.19.4, 4.18, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0010_scope_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let attribute = Attribute::from(Property::builder("/Barcelona").build()?);
    // The model refuses a name or an id of the path; the operation is built for a placeholder and sent to the path as written.
    let request = broker
        .unchecked(
            &ReplaceAttribute::builder()
                .id(&building_id)
                .name(&"placeholder".parse::<AttributeName>()?)
                .attribute(&attribute)
                .build(),
        )?
        .uri(api_uri(&broker, &format!("entities/{building_id}/attrs/scope"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    Ok(())
}
