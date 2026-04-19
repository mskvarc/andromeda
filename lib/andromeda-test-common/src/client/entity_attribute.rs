//! `/entities/{id}/attrs` endpoint wrappers.

use crate::{
    client::{NgsiLdClient, NgsiLdResponse, raw::response_into_ngsi_ld},
    header::jsonld_link_header,
};
use serde_json::Value;
use urlencoding::encode;

impl NgsiLdClient {
    /// `POST /entities/{id}/attrs` — append attributes.
    pub async fn append_entity_attrs(&self, id: &str, fragment: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}/attrs", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().post(url).header("Content-Type", "application/ld+json").json(fragment));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PATCH /entities/{id}/attrs` — partial update of every attribute in
    /// the fragment (NGSI-LD 5.6.14).
    pub async fn update_entity_attrs(&self, id: &str, fragment: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}/attrs", self.base_url(), encode(id));
        let request = self.apply_tenant(self.http().patch(url).header("Content-Type", "application/ld+json").json(fragment));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `PATCH /entities/{id}/attrs/{attr}` — partial update of a single
    /// attribute (NGSI-LD 5.6.15).
    pub async fn partial_update_attr(&self, id: &str, attr: &str, fragment: &Value) -> Result<NgsiLdResponse, reqwest::Error> {
        let url = format!("{}/entities/{}/attrs/{}", self.base_url(), encode(id), encode(attr));
        let request = self.apply_tenant(self.http().patch(url).header("Content-Type", "application/ld+json").json(fragment));
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }

    /// `DELETE /entities/{id}/attrs/{attr}` — attribute removal (NGSI-LD 5.6.5).
    pub async fn delete_entity_attr(
        &self,
        id: &str,
        attr: &str,
        dataset_id: Option<&str>,
        delete_all: Option<bool>,
        context_url: Option<&str>,
    ) -> Result<NgsiLdResponse, reqwest::Error> {
        let mut query = Vec::<String>::new();
        if let Some(dataset_id) = dataset_id {
            query.push(format!("datasetId={}", encode(dataset_id)));
        }
        if let Some(delete_all) = delete_all {
            query.push(format!("deleteAll={delete_all}"));
        }
        let query_suffix = if query.is_empty() { String::new() } else { format!("?{}", query.join("&")) };
        let url = format!("{}/entities/{}/attrs/{}{query_suffix}", self.base_url(), encode(id), encode(attr));
        let mut request = self.http().delete(url);
        if let Some(context_url) = context_url {
            request = request.header("Link", jsonld_link_header(context_url));
        }
        let request = self.apply_tenant(request);
        let response = request.send().await?;
        response_into_ngsi_ld(response).await
    }
}
