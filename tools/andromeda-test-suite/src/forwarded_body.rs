//! The body of a request a broker forwarded to a context source, read as the document the forwarded operation
//! carries: an entity (Create Entity, Replace Entity), the entities of a batch, a fragment of attributes (Update
//! Attributes, Append Attributes, Merge Entity), one attribute (Partial Attribute Update), a temporal entity (Create or
//! Update Temporal Evolution), a fragment of evolutions (Add Attributes to Temporal Evolution), clause 5.6 forwarded
//! as clause 5.7.1.4 describes, or the copy of a subscription a broker places on a source (clause 5.8.1.4) and the
//! updates it sends that copy.
//!
//! The entities are read as a broker would answer them, so the entity and attribute matchers check them. A fragment
//! names no entity, so it is read as the attributes of an entity whose id and type are placeholders, and one attribute
//! as the only attribute of such an entity under the name its request path gives: the matchers then check the
//! attributes, and the placeholders are never compared.

use iri_rs::{Iri, iri};
use ngsi_ld::{
    model::{DocumentError, Entity, Input, Lossless, Output, evolution::TemporalEntity},
    subscription::{Problem, ReadErrorKind, Subscription, SubscriptionFragment},
};
use serde_json::{Map, Value};
use std::str;
use wiremock::Request;

/// The placeholder id of the entity holding a forwarded fragment's attributes.
const FRAGMENT_ID: Iri<&str> = iri!("urn:ngsi-ld:ForwardedFragment:placeholder");

/// The placeholder type of the entity holding a forwarded fragment's attributes.
const FRAGMENT_TYPE: &str = "ForwardedFragment";

/// Why a forwarded body is not the document the test reads.
#[derive(Debug, thiserror::Error)]
pub enum ForwardedBodyError {
    /// The body is not UTF-8 text.
    #[error("the forwarded body is not UTF-8: {0}")]
    Text(#[from] str::Utf8Error),
    /// The body is not the JSON shape the operation forwards.
    #[error("the forwarded body is not {expected}")]
    Shape {
        /// The shape the operation forwards.
        expected: JsonShape,
        /// Why the body does not read as that shape.
        #[source]
        source: serde_json::Error,
    },
    /// The model does not read the entity.
    #[error("the forwarded document is not an entity the model reads: {0}")]
    Entity(#[source] DocumentError),
    /// The model does not read the temporal entity.
    #[error("the forwarded document is not a temporal entity the model reads: {0}")]
    Evolution(#[source] DocumentError),
    /// The model does not read the subscription.
    #[error("the forwarded document is not a subscription the model reads: {0}")]
    Subscription(#[source] DocumentError<ReadErrorKind<Problem>>),
    /// The model does not read the subscription fragment.
    #[error("the forwarded document is not a subscription fragment the model reads: {0}")]
    SubscriptionFragment(#[source] DocumentError<ReadErrorKind<Problem>>),
}

/// The JSON shape a forwarded body has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum JsonShape {
    /// A JSON array.
    #[strum(serialize = "an array")]
    Array,
    /// A JSON object.
    #[strum(serialize = "an object")]
    Object,
}

/// The body of `request` as text.
fn text(request: &Request) -> Result<&str, ForwardedBodyError> {
    Ok(str::from_utf8(&request.body)?)
}

/// The entity `request` carries.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not an entity the model reads.
pub fn entity(request: &Request) -> Result<Entity<Output>, ForwardedBodyError> {
    Entity::<Output>::parse(text(request)?, Lossless::Normalized).map_err(ForwardedBodyError::Entity)
}

/// The entities of the batch `request` carries, in their order.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not an array of entities the model reads.
pub fn entities(request: &Request) -> Result<Vec<Entity<Output>>, ForwardedBodyError> {
    let body = text(request)?;
    let items: Vec<Value> = serde_json::from_str(body).map_err(|source| ForwardedBodyError::Shape {
        expected: JsonShape::Array,
        source,
    })?;
    items
        .iter()
        .map(|item| Entity::<Output>::parse(&item.to_string(), Lossless::Normalized).map_err(ForwardedBodyError::Entity))
        .collect()
}

/// The attributes of the fragment `request` carries, as the attributes of a placeholder entity.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not an object of attributes the model reads.
pub fn fragment(request: &Request) -> Result<Entity<Output>, ForwardedBodyError> {
    let body = text(request)?;
    let members: Map<String, Value> = serde_json::from_str(body).map_err(|source| ForwardedBodyError::Shape {
        expected: JsonShape::Object,
        source,
    })?;
    placeholder(members.into_iter().filter(|(name, _)| !matches!(name.as_str(), "id" | "type" | "@context")))
}

/// The attribute `request` carries, as the attribute `name` of a placeholder entity.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not one attribute the model reads.
pub fn attribute(request: &Request, name: &str) -> Result<Entity<Output>, ForwardedBodyError> {
    let body = text(request)?;
    let member: Map<String, Value> = serde_json::from_str(body).map_err(|source| ForwardedBodyError::Shape {
        expected: JsonShape::Object,
        source,
    })?;
    placeholder([(name.to_owned(), Value::Object(member))])
}

/// The temporal entity `request` carries.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not a temporal entity the model reads.
pub fn evolution(request: &Request) -> Result<TemporalEntity<Output>, ForwardedBodyError> {
    TemporalEntity::<Output>::parse(text(request)?, Lossless::Normalized).map_err(ForwardedBodyError::Evolution)
}

/// The evolutions of the temporal fragment `request` carries, as the evolutions of a placeholder temporal entity.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not an object of evolutions the model reads.
pub fn evolution_fragment(request: &Request) -> Result<TemporalEntity<Output>, ForwardedBodyError> {
    let body = text(request)?;
    let members: Map<String, Value> = serde_json::from_str(body).map_err(|source| ForwardedBodyError::Shape {
        expected: JsonShape::Object,
        source,
    })?;
    let wrapped = wrapped(members.into_iter().filter(|(name, _)| !matches!(name.as_str(), "id" | "type" | "@context")));
    TemporalEntity::<Output>::parse(&wrapped, Lossless::Normalized).map_err(ForwardedBodyError::Evolution)
}

/// The subscription copy `request` carries: a Create Subscription request (clause 5.8.1.4), read as the input
/// document it is.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not a subscription the model reads.
pub fn subscription(request: &Request) -> Result<Subscription<Input>, ForwardedBodyError> {
    Subscription::<Input>::parse(text(request)?).map_err(ForwardedBodyError::Subscription)
}

/// The update of a subscription copy `request` carries: an Update Subscription request (clause 5.8.2.4), read as the
/// fragment it is.
///
/// # Errors
///
/// Returns [`ForwardedBodyError`] when the body is not a subscription fragment the model reads.
pub fn subscription_fragment(request: &Request) -> Result<SubscriptionFragment, ForwardedBodyError> {
    SubscriptionFragment::parse(text(request)?).map_err(ForwardedBodyError::SubscriptionFragment)
}

/// The placeholder entity holding `attributes`.
fn placeholder(attributes: impl IntoIterator<Item = (String, Value)>) -> Result<Entity<Output>, ForwardedBodyError> {
    Entity::<Output>::parse(&wrapped(attributes), Lossless::Normalized).map_err(ForwardedBodyError::Entity)
}

/// The text of an object holding the placeholder id and type, then `members`.
fn wrapped(members: impl IntoIterator<Item = (String, Value)>) -> String {
    let mut wrapped = Map::from_iter([
        ("id".to_owned(), Value::from(FRAGMENT_ID.as_str())),
        ("type".to_owned(), Value::from(FRAGMENT_TYPE)),
    ]);
    wrapped.extend(members);
    Value::Object(wrapped).to_string()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::any_attribute,
        entity_matcher::{has_attribute, lacks_attribute},
        evolution_matcher::{has_evolution, lacks_evolution},
        forwarded_body::{attribute, entities, entity, evolution, evolution_fragment, fragment, subscription},
        nth::nth,
    };
    use googletest::prelude::*;
    use http::{HeaderMap, Method};
    use std::result::Result as StdResult;
    use url::{ParseError, Url};
    use wiremock::Request;

    /// A request forwarded to a source with `body`.
    fn forwarded(body: &str) -> StdResult<Request, ParseError> {
        Ok(Request {
            url: Url::parse("http://source.test/ngsi-ld/v1/entities")?,
            method: Method::POST,
            headers: HeaderMap::new(),
            body: body.as_bytes().to_vec(),
        })
    }

    #[gtest]
    fn an_entity_is_read_with_its_attributes() -> Result<()> {
        let request = forwarded(r#"{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","speed":{"type":"Property","value":5}}"#)?;
        verify_that!(entity(&request)?, has_attribute("speed", any_attribute().value(5)))
    }

    #[gtest]
    fn a_batch_is_read_entity_by_entity() -> Result<()> {
        let request = forwarded(r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle"}]"#)?;
        verify_that!(entities(&request)?, len(eq(2)))
    }

    #[gtest]
    fn a_fragment_is_read_as_the_attributes_of_a_placeholder_entity() -> Result<()> {
        let request = forwarded(r#"{"speed":{"type":"Property","value":7}}"#)?;
        let read = fragment(&request)?;
        expect_that!(&read, has_attribute("speed", any_attribute().value(7)));
        verify_that!(&read, lacks_attribute("brandName"))
    }

    #[gtest]
    fn one_attribute_is_read_under_the_name_of_its_path() -> Result<()> {
        let request = forwarded(r#"{"type":"Property","value":9}"#)?;
        verify_that!(attribute(&request, "speed")?, has_attribute("speed", any_attribute().value(9)))
    }

    #[gtest]
    fn a_temporal_entity_is_read_with_its_instances() -> Result<()> {
        let request = forwarded(r#"{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","speed":[{"type":"Property","value":42}]}"#)?;
        verify_that!(&evolution(&request)?, has_evolution("speed", nth(0, any_attribute().value(42))))
    }

    #[gtest]
    fn a_temporal_fragment_is_read_as_the_evolutions_of_a_placeholder_entity() -> Result<()> {
        let request = forwarded(r#"{"speed":[{"type":"Property","value":99},{"type":"Property","value":98}]}"#)?;
        let read = evolution_fragment(&request)?;
        expect_that!(&read, has_evolution("speed", len(eq(2))));
        verify_that!(&read, lacks_evolution("brandName"))
    }

    #[gtest]
    fn a_batch_body_that_is_not_an_array_is_refused() -> Result<()> {
        let request = forwarded(r#"{"id":"urn:ngsi-ld:Vehicle:A"}"#)?;
        verify_that!(entities(&request), err(anything()))
    }

    #[gtest]
    fn a_subscription_copy_is_read_with_its_id_and_query() -> Result<()> {
        let request = forwarded(
            r#"{"id":"urn:ngsi-ld:Subscription:copy","type":"Subscription","entities":[{"type":"https://example.org/Vehicle"}],"q":"speed>10","notification":{"endpoint":{"uri":"http://broker.test/notify"}}}"#,
        )?;
        let copy = subscription(&request)?;
        expect_that!(copy.id().map(ToString::to_string), some(eq("urn:ngsi-ld:Subscription:copy")));
        verify_that!(copy.q(), some(anything()))
    }

    #[gtest]
    fn a_subscription_fragment_without_its_members_is_refused() -> Result<()> {
        let request = forwarded(r#"{"description":"marker"}"#)?;
        verify_that!(subscription(&request), err(anything()))
    }
}
