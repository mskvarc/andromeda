//! `/entities` endpoint wrappers.

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::Value;
use urlencoding::encode;

/// Request builder for `POST /entities`.
pub struct CreateEntity<'a> {
    client: &'a NgsiLdClient,
    body: &'a Value,
    content_type: &'static str,
    link_context: Option<&'a str>,
}

impl<'a> CreateEntity<'a> {
    /// Use `application/ld+json` with the `@context` embedded in the body
    /// (the default).
    pub fn with_ld_json(mut self) -> CreateEntity<'a> {
        self.content_type = "application/ld+json";
        self.link_context = None;
        self
    }

    /// Use `application/json` with the `@context` supplied via a `Link`
    /// header (NGSI-LD 6.3.5).
    pub fn with_json(mut self, context_url: &'a str) -> CreateEntity<'a> {
        self.content_type = "application/json";
        self.link_context = Some(context_url);
        self
    }

    /// Send the request and return the decoded response.
    pub async fn send(self) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities", self.client.base_url());
        let mut request = self.client.http().post(url).header("Content-Type", self.content_type);
        if let Some(context_url) = self.link_context {
            request = request.header("Link", jsonld_link_header(context_url));
            let mut stripped = self.body.clone();
            if let Some(obj) = stripped.as_object_mut() {
                obj.remove("@context");
            }
            request = request.json(&stripped);
        } else {
            request = request.json(self.body);
        }
        request = self.client.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}

impl NgsiLdClient {
    /// Begin a `POST /entities` request with body `entity` and the default
    /// `application/ld+json` content type.
    pub fn post_entity<'a>(&'a self, entity: &'a Value) -> CreateEntity<'a> {
        CreateEntity {
            client: self,
            body: entity,
            content_type: "application/ld+json",
            link_context: None,
        }
    }

    /// `GET /entities/{id}` with query string `query`, optional `Link` header
    /// containing `context_url`. `query` may be empty.
    pub async fn get_entity(&self, id: &str, query: &str, accept: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/entities/{}{sep}{query}", self.base_url(), encode(id));
        let mut request = self.http().get(url).header("Accept", accept);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `GET /entities` with query string `query`.
    pub async fn query_entities(&self, query: &str, accept: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/entities{sep}{query}", self.base_url());
        let mut request = self.http().get(url).header("Accept", accept);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /entities/{id}`.
    pub async fn delete_entity(&self, id: &str) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().delete(url));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /entities?<query>` — collection delete by query (NGSI-LD 5.6.17).
    pub async fn delete_entities_by_query(&self, query: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let sep = if query.is_empty() { "" } else { "?" };
        let url = format!("{}/entities{sep}{query}", self.base_url());
        let mut request = self.http().delete(url);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PATCH /entities/{id}` — merge-patch (NGSI-LD 5.6.16).
    pub async fn merge_entity(&self, id: &str, fragment: &Value, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}", self.base_url(), encode(id));
        let content_type = if context_url.is_some() { "application/json" } else { "application/ld+json" };
        let mut request = self.http().patch(url).header("Content-Type", content_type);
        let body = if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
            let mut stripped = fragment.clone();
            if let Some(obj) = stripped.as_object_mut() {
                obj.remove("@context");
            }
            stripped
        } else {
            fragment.clone()
        };
        request = request.json(&body);
        request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PUT /entities/{id}` — replace (NGSI-LD 5.6.24).
    pub async fn replace_entity(&self, id: &str, entity: &Value, content_type: &str, context_url: Option<&str>) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}", self.base_url(), encode(id));
        let mut request = self.http().put(url).header("Content-Type", content_type);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let body = if content_type == "application/json" {
            let mut stripped = entity.clone();
            if let Some(obj) = stripped.as_object_mut() {
                obj.remove("@context");
            }
            stripped
        } else {
            entity.clone()
        };
        request = request.json(&body).header("Accept", "application/ld+json");
        request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}

/// Read the `NGSILD-Results-Count` response header as a `usize`, if present
/// and parseable.
pub fn results_count(headers: &HeaderMap) -> Option<usize> {
    headers
        .get("NGSILD-Results-Count")
        .and_then(|value| value.to_str().ok())
        .and_then(|s| s.parse().ok())
}

/// Assert that `status` is `StatusCode::CREATED`; return the `Location` header.
pub fn require_location(response: &NgsiLdResponse) -> String {
    pretty_assertions::assert_eq!(response.status, StatusCode::CREATED, "expected 201 Created");
    match response.location() {
        Some(location) => location,
        None => {
            eprintln!("expected Location header on 201 response; headers = {:?}", response.headers);
            std::process::exit(2)
        }
    }
}
