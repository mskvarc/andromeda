//! Starting a rendering from what a request handler holds.
//!
//! `andromeda-ngsi-ld-render` takes values: a media type, a resolved
//! `@context`, and the `Prefer` tokens that change how a payload is written.
//! A handler holds an axum extractor and a [`ContextSpec`], and this is the one
//! place those are turned into the other. Nothing else in the tree resolves a
//! `ContextSpec` for presentation, so the body value and the `Link` URLs cannot
//! come from two different readings of it.

use crate::http::extractor::prefer::PreferHeader;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_http_headers::media_type::ResponseMediaType;
use andromeda_ngsi_ld_render::{context::ResponseContext, response::ResponseRenderer};

/// Configure a response renderer from a handler's media type, context and preferences.
///
/// Returns the renderer before its payload; the caller attaches that with
/// [`ResponseRenderer::with_payload`], which is the only route to a runnable
/// response, so a handler cannot render a body without one.
pub fn start_rendering<'a>(media_type: ResponseMediaType, context: &ContextSpec, prefer: PreferHeader) -> ResponseRenderer<'a> {
    ResponseRenderer::new(media_type, response_context(context))
        .with_geo_json_body(prefer.geo_json_body)
        .with_requested_version(prefer.requested_version)
}

/// Resolve a context spec into the two spellings a response needs.
///
/// Both come from a single [`ContextSpec::for_presentation`] reading, so a
/// response cannot advertise one set of contexts in its `Link` header and embed
/// a different set in its body.
pub fn response_context(context: &ContextSpec) -> ResponseContext {
    let presentation = context.for_presentation();
    ResponseContext::new(presentation.to_context_value(), presentation.to_link_headers())
}

#[cfg(test)]
mod tests {
    use crate::http::response::render::response_context;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriRefBuf;

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_string()).expect("a valid IRI reference")
    }

    #[test]
    fn the_body_value_and_the_link_urls_come_from_one_presentation_reading() {
        // Two `for_presentation()` calls used to resolve this, one in the
        // renderer and one in the header builder. They agree here by
        // construction rather than by both being written the same way.
        let spec = ContextSpec::from_links(vec![iri_ref("https://example.com/context.jsonld")]);
        let presentation = spec.for_presentation();
        let mut resolved = response_context(&spec);

        pretty_assertions::assert_eq!(resolved.take_body_value(), presentation.to_context_value());
        pretty_assertions::assert_eq!(resolved.link_urls(), presentation.to_link_headers().as_slice());
    }

    #[test]
    fn a_spec_with_no_explicit_context_still_advertises_the_core_context() {
        let resolved = response_context(&ContextSpec::none());
        assert!(
            resolved.link_urls().iter().any(|url| NGSI_LD_CORE_CONTEXT_URL == url.as_str()),
            "the core context is missing from {:?}",
            resolved.link_urls()
        );
    }
}
