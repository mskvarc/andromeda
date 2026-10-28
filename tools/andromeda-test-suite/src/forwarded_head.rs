//! The header fields of a request a broker forwarded to a context source, read through the typed headers a response
//! is read with: the `Via` a forwarding broker adds (clause 6.3.24), the JSON-LD context `Link` (clause 6.3.5), the
//! `Content-Type` and the `NGSILD-EntityMap` of a distributed query (clause 6.3.23).

use ngsi_ld::http::{
    ResponseHead,
    headers::{ReceivedBy, Via},
};
use wiremock::Request;

/// The header fields of `request`, read with `ResponseHead::typed`.
#[must_use]
pub fn head(request: &Request) -> ResponseHead {
    // Clone: the head owns its fields, and the source's journal keeps the request.
    ResponseHead::new(request.headers.clone())
}

/// The hosts the `Via` of `request` names as having received it (RFC 9110 section 7.6.3), in order; `None` when the
/// request carries no `Via` that reads.
#[must_use]
pub fn via_hosts(request: &Request) -> Option<Vec<ReceivedBy>> {
    head(request)
        .typed::<Via>()
        .valid()
        .map(|via| via.entries().iter().map(|entry| entry.received_by().clone()).collect())
}

#[cfg(test)]
mod tests {
    use crate::forwarded_head::{head, via_hosts};
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderValue, Method};
    use ngsi_ld::{
        http::headers::{ContentType, ReceivedBy},
        model::NgsiLdMediaType,
    };
    use url::Url;
    use wiremock::Request;

    /// A forwarded request carrying `headers`.
    fn forwarded(headers: &[(&'static str, &'static str)]) -> Result<Request> {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.append(*name, HeaderValue::from_static(value));
        }
        Ok(Request {
            url: Url::parse("http://source.test/ngsi-ld/v1/entities")?,
            method: Method::GET,
            headers: map,
            body: Vec::new(),
        })
    }

    #[gtest]
    fn the_via_hosts_are_read_in_order() -> Result<()> {
        let request = forwarded(&[("via", "1.1 broker-a, 1.1 broker-b")])?;
        let hosts = via_hosts(&request);
        verify_that!(
            hosts,
            some(elements_are![eq(&"broker-a".parse::<ReceivedBy>()?), eq(&"broker-b".parse::<ReceivedBy>()?)])
        )
    }

    #[gtest]
    fn a_request_without_via_names_no_host_and_its_content_type_reads() -> Result<()> {
        let request = forwarded(&[("content-type", "application/ld+json")])?;
        expect_that!(via_hosts(&request), none());
        verify_that!(
            head(&request).typed::<ContentType>().valid(),
            some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
        )
    }
}
