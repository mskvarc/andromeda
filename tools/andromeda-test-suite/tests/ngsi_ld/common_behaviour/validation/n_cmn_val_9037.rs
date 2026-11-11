//! An ngsildproof whose value is no proof is bad request data on every write.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Entity, ErrorType, Value};

/// The values written as the value of an ngsildproof, each of which is no JSON object holding a proof: clause 4.5.2.2
/// makes the value "an object containing the W3C Data integrity proof structure".
const VALUES: [&str; 5] = ["42", r#""proof""#, "[1, 2]", "null", "{}"];

/// How the ngsildproof is spelled inside the attribute it seals.
#[derive(Debug, Clone, Copy)]
enum Spelling {
    /// A normalized Property naming its type.
    Normalized,
    /// A concise Property: its value and seals without a type.
    Concise,
    /// The value alone, which the concise representation reads as a Property holding it.
    Bare,
}

/// The ngsildproof holding `value`, spelled `spelling` and sealed to `id` of `entity_type`.
fn seal(spelling: Spelling, value: &str, id: &str, entity_type: &str) -> String {
    match spelling {
        Spelling::Normalized => format!(r#"{{"type": "Property", "entityIdSealed": "{id}", "entityTypeSealed": "{entity_type}", "value": {value}}}"#),
        Spelling::Concise => format!(r#"{{"entityIdSealed": "{id}", "entityTypeSealed": "{entity_type}", "value": {value}}}"#),
        Spelling::Bare => value.to_owned(),
    }
}

/// Pins: Create Entity, Append Attributes, Update Attributes, Merge Entity and Replace Attribute writing an attribute
/// whose ngsildproof holds a number, a string, an array, null or an empty object answer 400 `BadRequestData`, whether
/// the ngsildproof is written normalized, concise or as its bare value, and the Entity written to keeps its attribute
/// unsealed and gains none
/// Clause: 4.5.2.2, 5.6.1.4, 5.6.2.4, 5.6.3.4, 5.6.17.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_cmn_val_9037_an_ngsildproof_whose_value_is_no_proof_is_bad_request_data_on_every_write() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", "unsealed")?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    for (position, value) in VALUES.into_iter().enumerate() {
        for spelling in [Spelling::Normalized, Spelling::Concise, Spelling::Bare] {
            if matches!(spelling, Spelling::Bare) && value == "{}" {
                continue;
            }
            let sealed = seal(spelling, value, &building_id.to_string(), building_type.as_ref());
            let attribute = match spelling {
                Spelling::Normalized => format!(r#"{{"type": "Property", "value": 22.5, "ngsildproof": {sealed}}}"#),
                Spelling::Concise | Spelling::Bare => format!(r#"{{"value": 22.5, "ngsildproof": {sealed}}}"#),
            };
            let created_id = namespace.urn("Building", format!("created-{position}-{spelling:?}"))?;
            // The model refuses an ngsildproof that is no proof, so every body is sent as written.
            let writes = [
                (
                    "create",
                    Method::POST,
                    "entities".to_owned(),
                    format!(r#"{{"id": "{created_id}", "type": "{building_type}", "temperature": {attribute}}}"#),
                ),
                (
                    "append",
                    Method::POST,
                    format!("entities/{building_id}/attrs"),
                    format!(r#"{{"humidity": {attribute}}}"#),
                ),
                (
                    "update",
                    Method::PATCH,
                    format!("entities/{building_id}/attrs"),
                    format!(r#"{{"temperature": {attribute}}}"#),
                ),
                (
                    "merge",
                    Method::PATCH,
                    format!("entities/{building_id}"),
                    format!(r#"{{"temperature": {attribute}}}"#),
                ),
                ("replace", Method::PUT, format!("entities/{building_id}/attrs/temperature"), attribute.clone()),
            ];
            for (operation, method, path, body) in writes {
                let answer = broker.send_unchecked(json_request_to(&broker, method, &path, &body)?).await;
                expect_that!(
                    &answer,
                    problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
                    "{operation} of an ngsildproof spelled {spelling:?} holding {value}"
                );
            }
        }
    }
    let retrieved = broker.entities().retrieve(&building_id).send().await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("temperature", property().value(Value::Number("21.5".parse()?)).without_proof())
    );
    expect_that!(entity, lacks_attribute("humidity"));
    Ok(())
}
