//! Language negotiation extractor for NGSI-LD requests.
//!
//! Resolves an optional [`LanguageFilter`] from the `Accept-Language` header
//! the [`crate::accept_language`] layer accepted. Query parameters are parsed
//! by the endpoint-specific request model so each wire value is decoded exactly
//! once, and the handler lets a `lang` outrank the header.

use crate::accept_language::AcceptedLanguage;
use andromeda_http_headers::language::LanguageFilter;
use axum::{extract::FromRequestParts, http::request::Parts};
use std::{
    convert::Infallible,
    future::{Future, ready},
};

/// Axum extractor that resolves an optional [`LanguageFilter`] from the request.
///
/// `None` when the `accept_language` extension is off or the header named no
/// language.
pub struct LanguageNegotiation(pub Option<LanguageFilter>);

impl<S> FromRequestParts<S> for LanguageNegotiation
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        ready(Ok(LanguageNegotiation(
            parts.extensions.remove::<AcceptedLanguage>().map(|AcceptedLanguage(filter)| filter),
        )))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::language::LanguageNegotiation;
    use axum::{extract::FromRequestParts, http::Request};

    async fn extract(request: Request<()>) -> LanguageNegotiation {
        let (mut parts, ()) = request.into_parts();
        match LanguageNegotiation::from_request_parts(&mut parts, &()).await {
            Ok(negotiation) => negotiation,
            Err(never) => match never {},
        }
    }

    #[tokio::test]
    async fn query_parameter_is_left_to_the_request_model() {
        let LanguageNegotiation(filter) = extract(Request::builder().uri("/entities?lang=fr").body(()).expect("valid request")).await;
        assert!(filter.is_none());
    }

    #[tokio::test]
    async fn returns_none_when_absent() {
        let LanguageNegotiation(filter) = extract(Request::builder().uri("/entities").body(()).expect("valid request")).await;
        assert!(filter.is_none());
    }
}
