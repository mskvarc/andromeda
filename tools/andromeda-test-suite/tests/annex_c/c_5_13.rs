//! Annex C.5.13: a query filtering one language of a `LanguageProperty` and asking another answers the Vehicle in the
//! language asked for (clauses 4.9 and 4.15).

use crate::{
    printed_document::expect_flat,
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{SelectorType, language_filter, q::Query},
};
use serde_json::json;

/// Pins: Query Entities filtering marque[en] and asking lang=de answers the Vehicle with its German marque as C.5.13
/// prints; the printed marque[en-GB] selector is outside the 4.9 grammar, so it is refused as `BadRequestData`
/// Clause: C.5.13, 4.9, 4.15, 5.7.2
/// Expected: the filter is marque[en], the clause 4.9 grammar admitting no - in the selector, and marque[en-GB] is
/// refused
#[gtest]
#[tokio::test]
async fn c_5_13_0001_a_language_filtered_query_answers_the_german_marque() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", "A4567")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .language_property("marque", [("en-GB", "Vauxhall Viva"), ("en", "Vauxhall Viva"), ("de", "Opel Karl")])
        .build()?;
    broker.entities().create(&vehicle).send().await?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .query_simplified()
            .entity_type(namespace.suffixed("Vehicle").parse::<SelectorType>()?)
            .attrs(vec!["marque".parse()?])
            .q(r#"marque[en]=="Vauxhall Viva""#.parse::<Query>()?)
            .lang(language_filter::language("de").build()?)
            .send()
            .await,
    )?;
    expect_flat(
        body,
        vec![json!({"id": vehicle.id.to_string(), "type": namespace.suffixed("Vehicle"), "marque": "Opel Karl"})],
    )?;
    // The client refuses a q outside the 4.9 grammar, so the printed selector is written into the target.
    let target = format!(
        "entities?type={}&attrs=marque&q=marque%5Ben-GB%5D%3D%3D%22Vauxhall%20Viva%22&format=simplified&lang=de",
        namespace.suffixed("Vehicle")
    );
    let request = wire::request_to(&broker, Method::GET, &target)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let refused = broker.send_unchecked(request).await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    served.stop();
    Ok(())
}
