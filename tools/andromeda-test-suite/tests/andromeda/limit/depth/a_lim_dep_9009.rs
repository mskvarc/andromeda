//! Every write of an existing entity refuses an attribute past the limit.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    value::object,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, EntityFragment, ErrorType, NgsiLdMediaType, Property, Value},
};

/// Pins: Replace Entity, Update Attributes, Append Attributes, Partial Attribute Update, Replace Attribute and
/// Merge Entity each with p nesting 33 levels answer 400 `BadRequestData` naming the limit, and the entity keeps p
/// of one level
/// Clause: 5.6.18.4, 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.19.4, 5.6.17.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9009_every_write_of_an_existing_entity_refuses_an_attribute_past_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property("p", object([("k", Value::from("leaf"))]))
        .build()?;
    broker.entities().create(&nesting_probe).send().await?.done()?;
    let nesting_probe_replacement = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type.clone())
        .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
        .build()?;
    let answer = broker.with_responses().entities().replace(&nesting_probe_replacement).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let fragment = EntityFragment::builder()
        .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
        .build()?;
    let answer_2 = broker.with_responses().entities().update_attributes(&nesting_probe_id, &fragment).send().await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let fragment_2 = EntityFragment::builder()
        .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
        .build()?;
    let answer_3 = broker
        .with_responses()
        .entities()
        .append_attributes(&nesting_probe_id, &fragment_2)
        .send()
        .await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    // The model refuses this attribute, so the body is sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{nesting_probe_id}/attrs/p"))?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"value\": {{\n        \"k\": {{\n            \"k\": {{\n                \"k\": {{\n                    \"k\": {{\n                        \"k\": {{\n                            \"k\": {{\n                                \"k\": {{\n                                    \"k\": {{\n{}\"k\": {{\n                                            \"k\": {{\n                                                \"k\": {{\n                                                    \"k\": {{\n                                                        \"k\": {{\n                                                            \"k\": {{\n                                                                \"k\": {{\n                                                                    \"k\": {{\n                                                                        \"k\": {{\n                                                                            \"k\": {{\n                                                                                \"k\": {{\n                                                                                    \"k\": {{\n                                                                                        \"k\": {{\n                                                                                            \"k\": {{\n                                                                                                \"k\": {{\n                                                                                                    \"k\": {{\n                                                                                                        \"k\": {{\n                                                                                                            \"k\": {{\n                                                                                                                \"k\": {{\n                                                                                                                    \"k\": {{\n                                                                                                                        \"k\": {{\n                                                                                                                            \"k\": {{\n                                                                                                                                \"k\": {{\n                                                                                                                                    \"k\": {{\n                                                                                                                                        \"k\": \"leaf\"\n                                                                                                                                    }}\n                                                                                                                                }}\n                                                                                                                            }}\n                                                                                                                        }}\n                                                                                                                    }}\n                                                                                                                }}\n                                                                                                            }}\n                                                                                                        }}\n                                                                                                    }}\n                                                                                                }}\n                                                                                            }}\n                                                                                        }}\n                                                                                    }}\n                                                                                }}\n                                                                            }}\n                                                                        }}\n                                                                    }}\n                                                                }}\n                                                            }}\n                                                        }}\n                                                    }}\n                                                }}\n                                            }}\n                                        }}\n                                    }}\n                                }}\n                            }}\n                        }}\n                    }}\n                }}\n            }}\n        }}\n    }}\n}}", " ".repeat(40))));
    let answer_4 = broker.send_unchecked(request).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let attribute = Attribute::from(Property::builder((0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)]))).build()?);
    let answer_5 = broker
        .with_responses()
        .entities()
        .replace_attribute(&nesting_probe_id, &"p".parse::<AttributeName>()?, &attribute)
        .send()
        .await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let fragment_3 = EntityFragment::builder()
        .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
        .build()?;
    let answer_6 = broker.with_responses().entities().merge(&nesting_probe_id, &fragment_3).send().await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("p", any_attribute().value_member(["k"], "leaf").without_value_member(["k", "k"]))
    );
    Ok(())
}
