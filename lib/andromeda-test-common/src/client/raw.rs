//! Low-level HTTP helpers for negative-path tests.
//!
//! The typed endpoint wrappers in this module's siblings strip `@context`
//! on `application/json` requests, inject the scope's tenant header, and
//! decode JSON bodies into [`Value`]. Negative-path tests bypass all of
//! that and want raw bytes in and raw bytes out; [`NgsiLdClient::raw`]
//! returns a [`RawBuilder`] that exposes the full `reqwest::RequestBuilder`
//! surface with only the tenant header still injected.

use crate::client::{NgsiLdClient, NgsiLdResponse};
use reqwest::{Method, RequestBuilder, Response};
use serde_json::Value;

/// Builder wrapping a raw `reqwest::RequestBuilder`.
pub struct RawBuilder<'a> {
    client: &'a NgsiLdClient,
    request: RequestBuilder,
}

impl<'a> RawBuilder<'a> {
    /// Attach a header to the outgoing request. Forwarded to `reqwest`.
    pub fn header(mut self, name: &str, value: &str) -> RawBuilder<'a> {
        self.request = self.request.header(name, value);
        self
    }

    /// Attach a JSON body. Forwarded to `reqwest::RequestBuilder::json`.
    pub fn json(mut self, body: &Value) -> RawBuilder<'a> {
        self.request = self.request.json(body);
        self
    }

    /// Attach a raw byte body.
    pub fn body(mut self, body: Vec<u8>) -> RawBuilder<'a> {
        self.request = self.request.body(body);
        self
    }

    /// Send the request and decode the response as JSON (falling back to
    /// `Value::Null` if the body is empty or not valid JSON).
    pub async fn send(self) -> Result<NgsiLdResponse, reqwest::Error> {
        let request = self.client.apply_tenant(self.request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}

impl NgsiLdClient {
    /// Return a raw request builder for `method` against `path`, which must
    /// start with `/ngsi-ld/v1/...`.
    pub fn raw(&self, method: Method, path: &str) -> RawBuilder<'_> {
        let url = format!("{}{path}", self.base_url().trim_end_matches("/ngsi-ld/v1"));
        let request = self.http().request(method, url);
        RawBuilder { client: self, request }
    }
}

pub(crate) async fn response_into_ngsi_ld(response: Response) -> Result<NgsiLdResponse, reqwest::Error> {
    let status = response.status();
    let headers = response.headers().clone();
    let body = match response.json::<Value>().await {
        Ok(value) => value,
        Err(_) => Value::Null,
    };
    Ok(NgsiLdResponse { status, headers, body })
}
