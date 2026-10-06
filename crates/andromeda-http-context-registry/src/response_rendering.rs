//! Starting a response's rendering under the `@context` its request named.
//!
//! Every NGSI-LD response that carries a body names the context of the request
//! it answers (clauses 5.2.3, 4.4 and 5.5.7), embedded in an
//! `application/ld+json` body or advertised by one `Link` header, and a
//! compound context is named through a wrapper hosted among the tenant's
//! stored `@context`s (Annex C.8, clause 5.13.1). That is three steps --
//! presenting the request context through the tenant's engine, hosting it
//! under the request's `apiRoot` when a `Link` must name it, and configuring
//! the renderer -- and [`ResponseContextSource`] is the one place they are
//! taken in that order, so no handler repeats them.
//!
//! It lives here rather than beside the renderer in `andromeda-http-response`
//! because two of its inputs, the tenant's hosting and the request's
//! `apiRoot`, are this crate's and the extraction crate's, and both of those
//! already sit above the response crate.

use crate::tenant_context_hosting::TenantContextHosting;
use andromeda_http_extraction::api_root::ApiRoot;
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    context_hosting_failure::classify_context_hosting_error,
    jsonld_failure::classify_jsonld_error,
    render::{RenderingPreferences, start_rendering},
};
use andromeda_jsonld::{compound_context_host::ContextHostingError, context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError};
use andromeda_ngsi_ld_render::response::ResponseRenderer;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use thiserror::Error;

/// Why a response could not name the context of the request it answers.
#[derive(Debug, Error)]
pub enum ResponseContextError {
    /// The request context could not be presented: a document it names could
    /// not be loaded (clause 5.5.6).
    #[error(transparent)]
    Presentation(#[from] JsonLdError),
    /// The compound request context could not be hosted for the `Link` that
    /// names it (Annex C.8).
    #[error(transparent)]
    Hosting(#[from] ContextHostingError),
}

/// Classify a response context that could not be named.
///
/// An unloadable document is the `LdContextNotAvailable` the same document
/// draws while a request is processed, and a hosting failure is classified as
/// every hosting failure is.
#[must_use]
pub fn classify_response_context_error(error: &ResponseContextError) -> ClassifiedReport {
    match error {
        ResponseContextError::Presentation(source) => classify_jsonld_error(source),
        ResponseContextError::Hosting(source) => classify_context_hosting_error(source),
    }
}

/// What a response's `@context` is resolved from: the tenant's engine, the
/// request's context, the tenant's hosting, and the `apiRoot` a hosted wrapper
/// is served under.
pub struct ResponseContextSource<'a> {
    /// The tenant's JSON-LD engine, which presents the request context.
    engine: &'a JsonLdEngine,
    /// The context the request named (clause 5.5.7).
    context: &'a ContextSpec,
    /// The tenant's stored `@context`s, which host a compound context.
    hosting: &'a TenantContextHosting,
    /// The `apiRoot` a hosted wrapper's URL is built on (clause 6.2).
    api_root: &'a ApiRoot,
}

impl<'a> ResponseContextSource<'a> {
    /// The source a response to a request naming `context` resolves its
    /// `@context` from.
    #[must_use]
    pub const fn new(
        engine: &'a JsonLdEngine,
        context: &'a ContextSpec,
        hosting: &'a TenantContextHosting,
        api_root: &'a ApiRoot,
    ) -> ResponseContextSource<'a> {
        ResponseContextSource {
            engine,
            context,
            hosting,
            api_root,
        }
    }

    /// Configure a renderer for a payload already rendered in `format`.
    ///
    /// # Errors
    ///
    /// Returns the [`ResponseContextError`] of a request context that could
    /// not be presented or hosted.
    pub async fn start_rendering<'r>(
        &self,
        media_type: ResponseMediaType,
        preferences: RenderingPreferences,
        format: ResponseFormat,
    ) -> Result<ResponseRenderer<'r>, ResponseContextError> {
        let presented = self.engine.presented_context(self.context).await?;
        Ok(start_rendering(media_type, presented, &self.hosting.for_request(self.api_root), preferences, format).await?)
    }

    /// Configure a renderer for a resource that has only the normalized
    /// representation (see
    /// [`andromeda_http_response::render::start_rendering_normalized`]).
    ///
    /// # Errors
    ///
    /// As [`ResponseContextSource::start_rendering`].
    pub async fn start_rendering_normalized<'r>(
        &self,
        media_type: ResponseMediaType,
        preferences: RenderingPreferences,
    ) -> Result<ResponseRenderer<'r>, ResponseContextError> {
        self.start_rendering(media_type, preferences, ResponseFormat::Normalized).await
    }
}

#[cfg(test)]
mod tests {
    use crate::response_rendering::{ResponseContextError, classify_response_context_error};
    use andromeda_jsonld::{compound_context_host::ContextHostingError, error::JsonLdError};
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;

    /// Clause 5.5.6: a document the request context names that cannot be
    /// loaded is `LdContextNotAvailable`, whichever step met it.
    #[test]
    fn an_unloadable_request_context_is_ld_context_not_available() {
        let error = ResponseContextError::from(JsonLdError::ContextFetchDisabled {
            url: IriBuf::new("https://example.com/ctx.jsonld".to_owned()).expect("a valid IRI"),
        });
        pretty_assertions::assert_eq!(classify_response_context_error(&error).error_type(), NgsiLdErrorType::LdContextNotAvailable);
    }

    #[test]
    fn a_hosting_failure_is_classified_as_every_hosting_failure_is() {
        let saturated = ResponseContextError::from(ContextHostingError::Persistence(PersistenceFailure::Saturated));
        pretty_assertions::assert_eq!(
            classify_response_context_error(&saturated).classification(),
            ProblemClassification::ServiceUnavailable
        );
        let fault = ResponseContextError::from(ContextHostingError::Persistence(PersistenceFailure::StatementRejected));
        pretty_assertions::assert_eq!(classify_response_context_error(&fault).error_type(), NgsiLdErrorType::InternalError);
    }
}
