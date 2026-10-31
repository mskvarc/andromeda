//! Serve @context sent with the header fields a conditional request carries (RFC 9110 sections 13.1 and 9.3.2): the
//! entity tag, the validators and the `Vary` of a stored context, which the typed serve neither writes nor reads.
//!
//! The typed `serve` reads only a 200 answer (clause 6.30.3.1), so a `304 Not Modified` answer, a `HEAD` request and
//! a request naming `If-None-Match` or `Accept-Encoding` are sent as unchecked requests to the path of the stored
//! context and read through the matchers of [`problem`](crate::problem).

use crate::{
    broker::BrokerClient,
    context_record::{ContextRecordError, segment_of_location},
    hosted_context::HostedContext,
    unchecked_page::Answer,
    wire::{WireError, header_value, header_values, request_to},
};
use bytes::Bytes;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Response,
    header::{ACCEPT, InvalidHeaderValue, VARY},
};
use ngsi_ld::model::{DocumentError, LocalContext};
use std::str::{self, Utf8Error};

/// Why a stored context could not be served as a probe.
#[derive(Debug, thiserror::Error)]
pub enum ServedContextError {
    /// The request could not be built.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The stored context's location names no identifier.
    #[error(transparent)]
    Location(#[from] ContextRecordError),
    /// A header value is not a header value.
    #[error("the header value is not valid")]
    HeaderValue(#[from] InvalidHeaderValue),
    /// The answer does not carry the header on exactly one field line.
    #[error("the answer carries {0} on {1} field lines, not one")]
    Lines(HeaderName, usize),
    /// The header value is not text.
    #[error("the header {0} is not text")]
    NotText(HeaderName),
    /// The body is not UTF-8.
    #[error("the served document is not UTF-8")]
    Utf8(#[from] Utf8Error),
    /// The body is not a local context document.
    #[error(transparent)]
    Document(#[from] DocumentError),
}

/// A stored context, served with header fields of the test's choosing.
#[derive(Debug)]
pub struct ServedContext<'c> {
    /// The client the requests go through.
    client: &'c BrokerClient,
    /// The path segment naming the context below `jsonldContexts/`.
    segment: String,
}

impl<'c> ServedContext<'c> {
    /// The context `hosted` names, as the `Location` of its creation wrote it.
    ///
    /// # Errors
    ///
    /// Returns [`ServedContextError::Location`] when the location holds no identifier.
    pub fn of(client: &'c BrokerClient, hosted: &HostedContext) -> Result<ServedContext<'c>, ServedContextError> {
        Ok(ServedContext {
            client,
            segment: segment_of_location(&hosted.location)?.to_owned(),
        })
    }

    /// The context at the path segment `segment`.
    #[must_use]
    pub fn at(client: &'c BrokerClient, segment: &str) -> ServedContext<'c> {
        ServedContext {
            client,
            segment: segment.to_owned(),
        }
    }

    /// Sends `method` to the context with `Accept: */*` and the header fields `headers`, as written.
    async fn send(&self, method: Method, query: &str, headers: &[(HeaderName, &str)]) -> Result<Answer, ServedContextError> {
        let mut request =
            request_to(self.client, method, &format!("jsonldContexts/{}{query}", self.segment))?.append_header(ACCEPT, HeaderValue::from_static("*/*"));
        for (name, value) in headers {
            request = request.append_header(name.clone(), HeaderValue::from_str(value)?);
        }
        Ok(self.client.send_unchecked(request).await)
    }

    /// Serve @context (`GET`) with the header fields `headers`.
    ///
    /// # Errors
    ///
    /// Returns [`ServedContextError`] when a header value is invalid; a broker's answer, an error included, is the
    /// `Ok` value.
    pub async fn get(&self, headers: &[(HeaderName, &str)]) -> Result<Answer, ServedContextError> {
        self.send(Method::GET, "", headers).await
    }

    /// `HEAD` of the context with the header fields `headers`.
    ///
    /// # Errors
    ///
    /// Returns [`ServedContextError`] when a header value is invalid.
    pub async fn head(&self, headers: &[(HeaderName, &str)]) -> Result<Answer, ServedContextError> {
        self.send(Method::HEAD, "", headers).await
    }

    /// Serve @context with `details=true`, answered as received.
    ///
    /// # Errors
    ///
    /// Returns [`ServedContextError`] when the request cannot be built.
    pub async fn details(&self) -> Result<Answer, ServedContextError> {
        self.send(Method::GET, "?details=true", &[]).await
    }
}

/// The text of the header `name` of `response`, which must sit on exactly one field line.
///
/// # Errors
///
/// Returns [`ServedContextError`] when the header is absent, split over lines or not text.
pub fn single_header(response: &Response<Bytes>, name: &HeaderName) -> Result<String, ServedContextError> {
    let lines = response.headers().get_all(name).iter().count();
    if lines != 1 {
        return Err(ServedContextError::Lines(name.clone(), lines));
    }
    header_value(response.headers(), name)
        .map(str::to_owned)
        .ok_or_else(|| ServedContextError::NotText(name.clone()))
}

/// The document a served answer carries.
///
/// # Errors
///
/// Returns [`ServedContextError`] when the body is not a local context document.
pub fn served_document(response: &Response<Bytes>) -> Result<LocalContext, ServedContextError> {
    Ok(LocalContext::parse(str::from_utf8(response.body())?)?)
}

/// Matches header fields whose first two `Vary` lines name `NGSILD-Tenant` and `Accept-Encoding`, one each, in
/// either order and in any case (RFC 9110 section 12.5.5).
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct VariesByTenantAndEncoding;

/// Header fields whose first two `Vary` lines name `NGSILD-Tenant` and `Accept-Encoding`, one each.
#[must_use]
pub const fn varies_by_tenant_and_encoding() -> VariesByTenantAndEncoding {
    VariesByTenantAndEncoding
}

/// Whether `line` is one of the two names the broker varies a stored context by.
const fn varied_name(line: &str) -> bool {
    line.eq_ignore_ascii_case("ngsild-tenant") || line.eq_ignore_ascii_case("accept-encoding")
}

impl Matcher<&HeaderMap> for VariesByTenantAndEncoding {
    fn matches(&self, actual: &HeaderMap) -> MatcherResult {
        match header_values(actual, &VARY).as_slice() {
            [first, second, ..] => (varied_name(first) && varied_name(second) && !first.eq_ignore_ascii_case(second)).into(),
            [] | [_] => MatcherResult::NoMatch,
        }
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => "varies by NGSILD-Tenant and Accept-Encoding on two field lines".into(),
            MatcherResult::NoMatch => "doesn't vary by NGSILD-Tenant and Accept-Encoding on two field lines".into(),
        }
    }

    fn explain_match(&self, actual: &HeaderMap) -> Description {
        format!("whose Vary lines are {:?}", header_values(actual, &VARY)).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::served_context::{single_header, varies_by_tenant_and_encoding};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{
        HeaderMap,
        HeaderValue,
        Response,
        header::{ETAG, VARY},
    };

    #[gtest]
    fn two_vary_lines_naming_each_variant_once_are_the_expected_variance() {
        let mut headers = HeaderMap::new();
        headers.append(VARY, HeaderValue::from_static("NGSILD-Tenant"));
        headers.append(VARY, HeaderValue::from_static("accept-encoding"));
        expect_that!(&headers, varies_by_tenant_and_encoding());
    }

    #[gtest]
    fn one_line_or_a_repeated_name_is_not() {
        let mut one = HeaderMap::new();
        one.append(VARY, HeaderValue::from_static("NGSILD-Tenant, Accept-Encoding"));
        expect_that!(&one, not(varies_by_tenant_and_encoding()));
        let mut twice = HeaderMap::new();
        twice.append(VARY, HeaderValue::from_static("NGSILD-Tenant"));
        twice.append(VARY, HeaderValue::from_static("ngsild-tenant"));
        expect_that!(&twice, not(varies_by_tenant_and_encoding()));
    }

    #[gtest]
    fn a_header_on_one_line_is_read_and_on_none_is_refused() -> Result<()> {
        let with = Response::builder().header(ETAG, HeaderValue::from_static("\"a\"")).body(Bytes::new())?;
        expect_that!(single_header(&with, &ETAG)?, eq("\"a\""));
        let without = Response::new(Bytes::new());
        verify_that!(single_header(&without, &ETAG).is_err(), eq(true))
    }
}
