//! The `@context` the name refusal tests write prefixed names under: the one prefix `ex` for `http://example.org/`,
//! carried in the body of every request, and the names and values that context expands to no IRI.
//!
//! JSON-LD 1.1 expansion drops a key that expands to no IRI, and clause 4.6.2 has a broker refuse a name outside its
//! grammar with `BadRequestData`, so a test writes one of these where a name or a type is expected and expects the
//! refusal.

use crate::broker::BrokerClient;
use ngsi_ld::{
    http::{ContextDelivery, EndpointError},
    model::{Context, ContextEntry, InlineContext, InlineContextError},
};

/// The prefix definition, the whole `@context` of a request.
const PREFIX: &str = r#"{"ex": "http://example.org/"}"#;

/// Names whose expansion under the prefix is no absolute IRI: an IRI admits no `{`, `"` or `%` that two hexadecimal
/// digits do not follow (IETF RFC 3987), and `urn` and `https` are no terms of the context.
pub const UNEXPANDABLE_NAMES: [&str; 6] = ["ex:a{b", "ex:{", "ex:%", "ex:\"", "urn:x:{", "https://example.org/{"];

/// Types and vocabulary terms whose expansion under the prefix is no absolute IRI, for the same reasons.
pub const UNEXPANDABLE_VALUES: [&str; 4] = ["ex:a{b", "ex:%", "urn:x:{", "https://example.org/{"];

/// The one name of [`UNEXPANDABLE_NAMES`] a test writes when the write, not the name, is its subject.
pub const UNEXPANDABLE_NAME: &str = "ex:a{b";

/// A prefixed type whose part after the prefix holds a hyphen, which the clause 4.6.2 grammar of a name refuses.
pub const HYPHENATED_TYPE: &str = "ex:bad-type";

/// Why the client writing under the prefix could not be made.
#[derive(Debug, thiserror::Error)]
pub enum ExamplePrefixError {
    /// The prefix definition is not an inline `@context`.
    #[error("the prefix definition is not an inline @context")]
    Context(#[from] InlineContextError),
    /// The client refused the `@context`.
    #[error("the client refused the @context")]
    Endpoint(#[from] EndpointError),
}

/// `client` writing under the prefix, which every request carries as its body's `@context` (clause 6.3.5).
///
/// # Errors
///
/// Returns [`ExamplePrefixError`] when the `@context` cannot be built or the client refuses it.
pub fn under_example_prefix(client: &BrokerClient) -> Result<BrokerClient, ExamplePrefixError> {
    let context = Context::Single(ContextEntry::Inline(PREFIX.parse::<InlineContext>()?));
    Ok(client.with_context(context)?.with_context_delivery(ContextDelivery::LdJsonBody))
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, example_prefix::under_example_prefix};
    use googletest::prelude::*;
    use iri_rs::iri;
    use ngsi_ld::{
        http::operation::CreateEntity,
        model::{Entity, EntityId},
    };
    use url::Url;

    #[gtest]
    fn a_write_carries_the_prefix_as_its_body_context() -> Result<()> {
        let client = under_example_prefix(&client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?)?;
        let vehicle = Entity::builder()
            .id(EntityId::from(iri!("urn:ngsi-ld:Vehicle:1")))
            .entity_type("Vehicle")
            .property("speed", 1)
            .build()?;
        let request = client.unchecked(&CreateEntity::builder().entity(&vehicle).build())?;
        let body = String::from_utf8_lossy(request.as_request().body()).into_owned();
        verify_that!(body, contains_substring(r#""@context":{"ex":"http://example.org/"}"#))
    }
}
