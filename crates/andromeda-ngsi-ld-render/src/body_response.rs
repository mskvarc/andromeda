//! The response pipeline for a typed resource that is not an entity: a
//! Subscription, a Context Source Registration, an `EntityMap`, a Snapshot, a
//! batch operation result or the type and attribute information of clauses
//! 6.25 to 6.28.
//!
//! The resource is written straight from its typed form into whatever the HTTP
//! binding writes the response into, with the clause 6.3.6 `@context` member
//! first when the response carries its context in the body. None of those
//! resources takes a `format`, holds a Relationship an entity renderer would
//! decode, or has a `GeoJSON` representation, so nothing else happens to the
//! body.

use crate::{
    context::ResponseContext,
    contextual_body::{ContextualBody, ContextualCollection},
    error::{RenderError, RenderedShape},
    response::{ResponseHead, ResponseRenderer},
};
use serde::Serialize;
use std::io::Write;

/// What a resource response answers with.
#[derive(Debug)]
pub enum BodyPayload<'b, T> {
    /// One resource.
    Single(&'b T),
    /// A collection of resources, each carrying its own `@context`.
    Collection(&'b [T]),
    /// A body that is no JSON-LD object (an array of identifiers, for one) and so
    /// carries no `@context` member whatever the response's carrier is.
    ContextFree(&'b T),
}

/// A renderer with its typed resource attached, ready to write.
#[derive(Debug)]
pub struct ReadyBodyResponse<'a, 'b, T> {
    /// The configuration the resource was attached to.
    renderer: ResponseRenderer<'a>,
    /// What is being answered.
    payload: BodyPayload<'b, T>,
}

impl<'a> ResponseRenderer<'a> {
    /// Attach one typed resource to write.
    #[must_use]
    pub const fn with_body<'b, T>(self, body: &'b T) -> ReadyBodyResponse<'a, 'b, T> {
        ReadyBodyResponse {
            renderer: self,
            payload: BodyPayload::Single(body),
        }
    }

    /// Attach a collection of typed resources to write, each carrying its own
    /// `@context` (clause 6.3.6).
    #[must_use]
    pub const fn with_collection<'b, T>(self, items: &'b [T]) -> ReadyBodyResponse<'a, 'b, T> {
        ReadyBodyResponse {
            renderer: self,
            payload: BodyPayload::Collection(items),
        }
    }

    /// Attach a body that is no JSON object, written as it is with no
    /// `@context` member, such as the array of identifiers a batch creation
    /// answers (clauses 5.6.7.5 and 5.6.8.5): an array is not a JSON-LD
    /// document, so there is no object the member could go on.
    #[must_use]
    pub const fn with_context_free_body<'b, T>(self, body: &'b T) -> ReadyBodyResponse<'a, 'b, T> {
        ReadyBodyResponse {
            renderer: self,
            payload: BodyPayload::ContextFree(body),
        }
    }
}

impl<T: Serialize> ReadyBodyResponse<'_, '_, T> {
    /// Write the response body into `writer` and say how the response
    /// carrying it is written.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the body cannot be written,
    /// including when `writer` fails.
    pub fn render_into<W: Write>(self, writer: W) -> Result<ResponseHead, RenderError> {
        let (context, context_link) = match &self.renderer.context {
            ResponseContext::Body(context) => (Some(context), None),
            ResponseContext::Link(target) => (None, Some(target.clone())),
            ResponseContext::Absent => (None, None),
        };
        let written = match self.payload {
            BodyPayload::Single(body) => serde_json::to_writer(writer, &ContextualBody::new(context, body)),
            BodyPayload::Collection(items) => serde_json::to_writer(writer, &ContextualCollection::new(context, items)),
            BodyPayload::ContextFree(body) => serde_json::to_writer(writer, &ContextualBody::new(None, body)),
        };
        written.map_err(|error| RenderError::serialization(RenderedShape::Resource, error))?;
        Ok(ResponseHead {
            context_link,
            media_type: self.renderer.media_type,
            version_adaptation: self.renderer.version_adaptation(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context::ResponseContext,
        response::{ResponseHead, ResponseRenderer},
        version::adaptation::VersionAdaptation,
    };
    use andromeda_http_headers::media_type::ResponseMediaType;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use iri_rs::IriBuf;
    use serde::Serialize;

    /// A resource serialised as a struct.
    #[derive(Serialize)]
    struct Resource {
        id: &'static str,
    }

    fn embedded() -> ResponseContext {
        ResponseContext::Body(JsonLiteral::admit(r#""https://example.org/ctx""#).expect("an admissible literal"))
    }

    fn linked() -> ResponseContext {
        ResponseContext::Link(IriBuf::new("https://example.org/ctx".to_owned()).expect("a valid IRI"))
    }

    /// The body and head a renderer writes for one resource.
    fn single(renderer: ResponseRenderer<'_>) -> (String, ResponseHead) {
        let mut written = Vec::new();
        let head = renderer.with_body(&Resource { id: "urn:a" }).render_into(&mut written).expect("a written body");
        (String::from_utf8(written).expect("UTF-8 JSON"), head)
    }

    #[test]
    fn an_embedded_context_is_written_first_and_not_advertised() {
        let (body, head) = single(ResponseRenderer::new(ResponseMediaType::JsonLd, embedded()));
        pretty_assertions::assert_eq!(body, r#"{"@context":"https://example.org/ctx","id":"urn:a"}"#);
        pretty_assertions::assert_eq!(head.context_link, None);
    }

    #[test]
    fn a_linked_context_is_advertised_and_not_written() {
        let (body, head) = single(ResponseRenderer::new(ResponseMediaType::Json, linked()));
        pretty_assertions::assert_eq!(body, r#"{"id":"urn:a"}"#);
        pretty_assertions::assert_eq!(head.context_link, Some(IriBuf::new("https://example.org/ctx".to_owned()).expect("a valid IRI")));
    }

    #[test]
    fn an_absent_context_is_neither_written_nor_advertised() {
        let (body, head) = single(ResponseRenderer::new(ResponseMediaType::Json, ResponseContext::Absent));
        pretty_assertions::assert_eq!(body, r#"{"id":"urn:a"}"#);
        pretty_assertions::assert_eq!(head.context_link, None);
    }

    #[test]
    fn a_collection_carries_the_context_on_every_item() {
        let mut written = Vec::new();
        let items = [Resource { id: "urn:a" }, Resource { id: "urn:b" }];
        ResponseRenderer::new(ResponseMediaType::JsonLd, embedded())
            .with_collection(&items)
            .render_into(&mut written)
            .expect("a written body");
        pretty_assertions::assert_eq!(
            String::from_utf8(written).expect("UTF-8 JSON"),
            r#"[{"@context":"https://example.org/ctx","id":"urn:a"},{"@context":"https://example.org/ctx","id":"urn:b"}]"#
        );
    }

    #[test]
    fn an_array_of_identifiers_is_written_without_a_context_member() {
        let mut written = Vec::new();
        let identifiers = ["urn:a", "urn:b"];
        let head = ResponseRenderer::new(ResponseMediaType::JsonLd, embedded())
            .with_context_free_body(&identifiers)
            .render_into(&mut written)
            .expect("a written body");
        pretty_assertions::assert_eq!(String::from_utf8(written).expect("UTF-8 JSON"), r#"["urn:a","urn:b"]"#);
        pretty_assertions::assert_eq!(head.context_link, None);
    }

    #[test]
    fn a_requested_older_version_is_reported_as_applied() {
        let (_, head) = single(ResponseRenderer::new(ResponseMediaType::Json, linked()).with_requested_version(Some(SpecVersion::V1_4)));
        pretty_assertions::assert_eq!(head.version_adaptation, VersionAdaptation::Applied { version: SpecVersion::V1_4 });
    }
}
