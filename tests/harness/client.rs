use reqwest::{
    Method,
    StatusCode,
    header::{HeaderMap, HeaderValue},
};

// ── RequestHeaders ──────────────────────────────────────────────────

/// Composable request headers. Replaces ad-hoc HeaderMap construction.
#[derive(Default, Clone, Debug)]
pub struct RequestHeaders(HeaderMap);

impl RequestHeaders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accept(mut self, media_type: &'static str) -> Self {
        self.0.insert("Accept", HeaderValue::from_static(media_type));
        self
    }

    pub fn accept_ld_json(self) -> Self {
        self.accept("application/ld+json")
    }

    pub fn accept_geo_json(self) -> Self {
        self.accept("application/geo+json")
    }

    pub fn link_context(mut self, context_url: &str) -> Self {
        let val = format!("<{context_url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
        self.0.insert("Link", HeaderValue::from_str(&val).unwrap());
        self
    }

    pub fn content_type(mut self, ct: &'static str) -> Self {
        self.0.insert("Content-Type", HeaderValue::from_static(ct));
        self
    }

    pub fn prefer(mut self, value: &str) -> Self {
        self.0.insert("Prefer", HeaderValue::from_str(value).unwrap());
        self
    }

    pub fn tenant(mut self, tenant: &str) -> Self {
        self.0.insert("NGSILD-Tenant", HeaderValue::from_str(tenant).unwrap());
        self
    }

    pub fn language(mut self, lang: &str) -> Self {
        self.0.insert("NGSILD-Language", HeaderValue::from_str(lang).unwrap());
        self
    }

    fn into_header_map(self) -> HeaderMap {
        self.0
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn has_content_type(&self) -> bool {
        self.0.contains_key("Content-Type") || self.0.contains_key("content-type")
    }
}

// ── NgsiLdResponse ──────────────────────────────────────────────────

pub struct NgsiLdResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    body: Vec<u8>,
}

impl NgsiLdResponse {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body).expect("failed to deserialize response body")
    }

    pub fn json_value(&self) -> serde_json::Value {
        self.json()
    }

    pub fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn location(&self) -> Option<&str> {
        self.headers.get("location")?.to_str().ok()
    }
}

// ── NgsiLdOption ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum NgsiLdOption {
    KeyValues,
    SysAttrs,
    TemporalValues,
    AggregatedValues,
}

impl NgsiLdOption {
    fn as_str(&self) -> &str {
        match self {
            Self::KeyValues => "keyValues",
            Self::SysAttrs => "sysAttrs",
            Self::TemporalValues => "temporalValues",
            Self::AggregatedValues => "aggregatedValues",
        }
    }
}

// ── QueryParams (builder pattern) ───────────────────────────────────

#[derive(Default, Debug, Clone)]
pub struct QueryParams {
    type_: Option<String>,
    id: Option<String>,
    id_pattern: Option<String>,
    attrs: Option<Vec<String>>,
    q: Option<String>,
    georel: Option<String>,
    geometry: Option<String>,
    coordinates: Option<String>,
    geoproperty: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    options: Option<Vec<NgsiLdOption>>,
    timerel: Option<String>,
    timeat: Option<String>,
    endtimeat: Option<String>,
    lastn: Option<u32>,
    scopeq: Option<String>,
    format: Option<String>,
    join: Option<String>,
    join_level: Option<u32>,
    lang: Option<String>,
    dataset_id: Option<String>,
    count: Option<bool>,
    geometry_property: Option<String>,
    timeproperty: Option<String>,
    aggr_methods: Option<String>,
    aggr_period_duration: Option<String>,
    delete_all: Option<bool>,
    local: Option<bool>,
    entity_map: Option<bool>,
    entity_map_lifetime: Option<String>,
    contained_by: Option<String>,
    split_entities: Option<bool>,
    order_by: Option<String>,
    order_from: Option<String>,
    order_geometry: Option<String>,
}

impl QueryParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn type_(mut self, v: impl Into<String>) -> Self {
        self.type_ = Some(v.into());
        self
    }

    pub fn id(mut self, v: impl Into<String>) -> Self {
        self.id = Some(v.into());
        self
    }

    pub fn id_pattern(mut self, v: impl Into<String>) -> Self {
        self.id_pattern = Some(v.into());
        self
    }

    pub fn attrs(mut self, v: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.attrs = Some(v.into_iter().map(Into::into).collect());
        self
    }

    pub fn q(mut self, v: impl Into<String>) -> Self {
        self.q = Some(v.into());
        self
    }

    pub fn georel(mut self, v: impl Into<String>) -> Self {
        self.georel = Some(v.into());
        self
    }

    pub fn geometry(mut self, v: impl Into<String>) -> Self {
        self.geometry = Some(v.into());
        self
    }

    pub fn coordinates(mut self, v: impl Into<String>) -> Self {
        self.coordinates = Some(v.into());
        self
    }

    pub fn geoproperty(mut self, v: impl Into<String>) -> Self {
        self.geoproperty = Some(v.into());
        self
    }

    pub fn limit(mut self, v: u32) -> Self {
        self.limit = Some(v);
        self
    }

    pub fn offset(mut self, v: u32) -> Self {
        self.offset = Some(v);
        self
    }

    pub fn options(mut self, v: impl IntoIterator<Item = NgsiLdOption>) -> Self {
        self.options = Some(v.into_iter().collect());
        self
    }

    pub fn timerel(mut self, v: impl Into<String>) -> Self {
        self.timerel = Some(v.into());
        self
    }

    pub fn timeat(mut self, v: impl Into<String>) -> Self {
        self.timeat = Some(v.into());
        self
    }

    pub fn endtimeat(mut self, v: impl Into<String>) -> Self {
        self.endtimeat = Some(v.into());
        self
    }

    pub fn lastn(mut self, v: u32) -> Self {
        self.lastn = Some(v);
        self
    }

    pub fn scopeq(mut self, v: impl Into<String>) -> Self {
        self.scopeq = Some(v.into());
        self
    }

    pub fn format(mut self, v: impl Into<String>) -> Self {
        self.format = Some(v.into());
        self
    }

    pub fn join(mut self, v: impl Into<String>) -> Self {
        self.join = Some(v.into());
        self
    }

    pub fn join_level(mut self, v: u32) -> Self {
        self.join_level = Some(v);
        self
    }

    pub fn lang(mut self, v: impl Into<String>) -> Self {
        self.lang = Some(v.into());
        self
    }

    pub fn dataset_id(mut self, v: impl Into<String>) -> Self {
        self.dataset_id = Some(v.into());
        self
    }

    pub fn count(mut self, v: bool) -> Self {
        self.count = Some(v);
        self
    }

    pub fn geometry_property(mut self, v: impl Into<String>) -> Self {
        self.geometry_property = Some(v.into());
        self
    }

    pub fn timeproperty(mut self, v: impl Into<String>) -> Self {
        self.timeproperty = Some(v.into());
        self
    }

    pub fn aggr_methods(mut self, v: impl Into<String>) -> Self {
        self.aggr_methods = Some(v.into());
        self
    }

    pub fn aggr_period_duration(mut self, v: impl Into<String>) -> Self {
        self.aggr_period_duration = Some(v.into());
        self
    }

    pub fn delete_all(mut self, v: bool) -> Self {
        self.delete_all = Some(v);
        self
    }

    pub fn local(mut self, v: bool) -> Self {
        self.local = Some(v);
        self
    }

    pub fn entity_map(mut self, v: bool) -> Self {
        self.entity_map = Some(v);
        self
    }

    pub fn entity_map_lifetime(mut self, v: impl Into<String>) -> Self {
        self.entity_map_lifetime = Some(v.into());
        self
    }

    pub fn contained_by(mut self, v: impl Into<String>) -> Self {
        self.contained_by = Some(v.into());
        self
    }

    #[allow(dead_code)]
    pub fn split_entities(mut self, v: bool) -> Self {
        self.split_entities = Some(v);
        self
    }

    pub fn order_by(mut self, v: impl Into<String>) -> Self {
        self.order_by = Some(v.into());
        self
    }

    pub fn order_from(mut self, v: impl Into<String>) -> Self {
        self.order_from = Some(v.into());
        self
    }

    pub fn order_geometry(mut self, v: impl Into<String>) -> Self {
        self.order_geometry = Some(v.into());
        self
    }

    fn enc(v: &str) -> String {
        urlencoding::encode(v).into_owned()
    }

    fn to_query_string(&self) -> String {
        let mut pairs: Vec<String> = Vec::new();
        if let Some(ref v) = self.type_ {
            pairs.push(format!("type={}", Self::enc(v)));
        }
        if let Some(ref v) = self.id {
            pairs.push(format!("id={v}"));
        }
        if let Some(ref v) = self.id_pattern {
            pairs.push(format!("idPattern={}", Self::enc(v)));
        }
        if let Some(ref v) = self.attrs {
            pairs.push(format!("attrs={}", Self::enc(&v.join(","))));
        }
        if let Some(ref v) = self.q {
            pairs.push(format!("q={v}"));
        }
        if let Some(ref v) = self.georel {
            pairs.push(format!("georel={}", Self::enc(v)));
        }
        if let Some(ref v) = self.geometry {
            pairs.push(format!("geometry={}", Self::enc(v)));
        }
        if let Some(ref v) = self.coordinates {
            pairs.push(format!("coordinates={}", Self::enc(v)));
        }
        if let Some(ref v) = self.geoproperty {
            pairs.push(format!("geoproperty={}", Self::enc(v)));
        }
        if let Some(v) = self.limit {
            pairs.push(format!("limit={v}"));
        }
        if let Some(v) = self.offset {
            pairs.push(format!("offset={v}"));
        }
        if let Some(ref opts) = self.options {
            let joined: Vec<&str> = opts.iter().map(|o| o.as_str()).collect();
            pairs.push(format!("options={}", Self::enc(&joined.join(","))));
        }
        if let Some(ref v) = self.timerel {
            pairs.push(format!("timerel={}", Self::enc(v)));
        }
        if let Some(ref v) = self.timeat {
            pairs.push(format!("timeAt={}", Self::enc(v)));
        }
        if let Some(ref v) = self.endtimeat {
            pairs.push(format!("endTimeAt={}", Self::enc(v)));
        }
        if let Some(v) = self.lastn {
            pairs.push(format!("lastN={v}"));
        }
        if let Some(ref v) = self.scopeq {
            pairs.push(format!("scopeQ={}", Self::enc(v)));
        }
        if let Some(ref v) = self.format {
            pairs.push(format!("format={}", Self::enc(v)));
        }
        if let Some(ref v) = self.join {
            pairs.push(format!("join={}", Self::enc(v)));
        }
        if let Some(v) = self.join_level {
            pairs.push(format!("joinLevel={v}"));
        }
        if let Some(ref v) = self.lang {
            pairs.push(format!("lang={}", Self::enc(v)));
        }
        if let Some(ref v) = self.dataset_id {
            pairs.push(format!("datasetId={}", Self::enc(v)));
        }
        if let Some(v) = self.count {
            pairs.push(format!("count={v}"));
        }
        if let Some(ref v) = self.geometry_property {
            pairs.push(format!("geometryProperty={}", Self::enc(v)));
        }
        if let Some(ref v) = self.timeproperty {
            pairs.push(format!("timeproperty={}", Self::enc(v)));
        }
        if let Some(ref v) = self.aggr_methods {
            pairs.push(format!("aggrMethods={}", Self::enc(v)));
        }
        if let Some(ref v) = self.aggr_period_duration {
            pairs.push(format!("aggrPeriodDuration={}", Self::enc(v)));
        }
        if let Some(v) = self.delete_all {
            pairs.push(format!("deleteAll={v}"));
        }
        if let Some(v) = self.local {
            pairs.push(format!("local={v}"));
        }
        if let Some(v) = self.entity_map {
            pairs.push(format!("entityMap={v}"));
        }
        if let Some(ref v) = self.entity_map_lifetime {
            pairs.push(format!("entityMapLifetime={}", Self::enc(v)));
        }
        if let Some(ref v) = self.contained_by {
            pairs.push(format!("containedBy={}", Self::enc(v)));
        }
        if let Some(v) = self.split_entities {
            pairs.push(format!("splitEntities={v}"));
        }
        if let Some(ref v) = self.order_by {
            pairs.push(format!("orderBy={}", Self::enc(v)));
        }
        if let Some(ref v) = self.order_from {
            pairs.push(format!("orderFrom={}", Self::enc(v)));
        }
        if let Some(ref v) = self.order_geometry {
            pairs.push(format!("orderGeometry={}", Self::enc(v)));
        }
        if pairs.is_empty() { String::new() } else { format!("?{}", pairs.join("&")) }
    }
}

// ── ContextQueryParams ──────────────────────────────────────────────

#[derive(Default, Debug)]
pub struct ContextQueryParams {
    details: Option<bool>,
    kind: Option<String>,
    reload: Option<bool>,
}

impl ContextQueryParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn details(mut self, v: bool) -> Self {
        self.details = Some(v);
        self
    }

    pub fn kind(mut self, v: impl Into<String>) -> Self {
        self.kind = Some(v.into());
        self
    }

    pub fn reload(mut self, v: bool) -> Self {
        self.reload = Some(v);
        self
    }

    fn to_query_string(&self) -> String {
        let mut pairs: Vec<String> = Vec::new();
        if let Some(v) = self.details {
            pairs.push(format!("details={v}"));
        }
        if let Some(ref v) = self.kind {
            pairs.push(format!("kind={v}"));
        }
        if let Some(v) = self.reload {
            pairs.push(format!("reload={v}"));
        }
        if pairs.is_empty() { String::new() } else { format!("?{}", pairs.join("&")) }
    }
}

// ── Content-Type detection ──────────────────────────────────────────

/// Returns the correct Content-Type for a JSON body.
fn content_type_for(body: &serde_json::Value) -> &'static str {
    let has_context = body.get("@context").is_some() || body.as_array().is_some_and(|arr| arr.iter().any(|v| v.get("@context").is_some()));
    if has_context { "application/ld+json" } else { "application/json" }
}

// ── NgsiLdClient ────────────────────────────────────────────────────

#[derive(Clone)]
pub struct NgsiLdClient {
    inner: reqwest::Client,
    pub base_url: String,
}

impl NgsiLdClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: reqwest::Client::new(),
            base_url: base_url.to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/ngsi-ld/v1{path}", self.base_url)
    }

    fn url_with_params(&self, path: &str, params: &QueryParams) -> String {
        format!("{}/ngsi-ld/v1{path}{}", self.base_url, params.to_query_string())
    }

    async fn send_response(resp: reqwest::Response) -> NgsiLdResponse {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.bytes().await.expect("failed to read response body");
        NgsiLdResponse {
            status,
            headers,
            body: body.to_vec(),
        }
    }

    async fn send_json(&self, req: reqwest::RequestBuilder, body: &serde_json::Value) -> NgsiLdResponse {
        let resp = req
            .header("Content-Type", content_type_for(body))
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .expect("request failed");
        Self::send_response(resp).await
    }

    // ── Generic request ──────────────────────────────────────────────

    pub async fn request(&self, method: Method, path: &str, body: Option<&serde_json::Value>, headers: RequestHeaders) -> NgsiLdResponse {
        let has_custom_ct = headers.has_content_type();
        let mut req = self.inner.request(method, self.url(path));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        if let Some(body) = body {
            if has_custom_ct {
                req = req.json(body);
                let resp = req.send().await.expect("request failed");
                return Self::send_response(resp).await;
            }
            return self.send_json(req, body).await;
        }
        let resp = req.send().await.expect("request failed");
        Self::send_response(resp).await
    }

    pub async fn request_raw(&self, method: Method, path: &str, body: &[u8], content_type: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .request(method, self.url(path))
            .header("Content-Type", content_type)
            .body(body.to_vec())
            .send()
            .await
            .expect("request_raw failed");
        Self::send_response(resp).await
    }

    // ── Entities ─────────────────────────────────────────────────────

    pub async fn create_entity(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entities")), body).await
    }

    pub async fn get_entity(&self, id: &str, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params(&format!("/entities/{id}"), params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("get_entity request failed");
        Self::send_response(resp).await
    }

    pub async fn query_entities(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/entities", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("query_entities request failed");
        Self::send_response(resp).await
    }

    pub async fn query_entities_with_entity_map(&self, params: &QueryParams, entity_map_id: &str, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/entities", params));
        let mut hm = headers.into_header_map();
        hm.insert("NGSILD-EntityMap", HeaderValue::from_str(entity_map_id).unwrap());
        req = req.headers(hm);
        let resp = req.send().await.expect("query_entities_with_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_entity(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/entities/{id}")))
            .send()
            .await
            .expect("delete_entity request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_entities_by_query(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.delete(self.url_with_params("/entities", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("delete_entities_by_query request failed");
        Self::send_response(resp).await
    }

    pub async fn merge_entity(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.patch(self.url(&format!("/entities/{id}"))), body).await
    }

    pub async fn replace_entity(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.put(self.url(&format!("/entities/{id}"))), body).await
    }

    // ── Entity Attributes ────────────────────────────────────────────

    pub async fn append_entity_attrs(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url(&format!("/entities/{id}/attrs"))), body).await
    }

    pub async fn update_entity_attrs(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.patch(self.url(&format!("/entities/{id}/attrs"))), body).await
    }

    pub async fn partial_update_attr(&self, id: &str, attr_id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.patch(self.url(&format!("/entities/{id}/attrs/{attr_id}"))), body)
            .await
    }

    pub async fn delete_entity_attr(&self, id: &str, attr_id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/entities/{id}/attrs/{attr_id}")))
            .send()
            .await
            .expect("delete_entity_attr request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_entity_attr_with_params(&self, id: &str, attr_id: &str, params: &QueryParams) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url_with_params(&format!("/entities/{id}/attrs/{attr_id}"), params))
            .send()
            .await
            .expect("delete_entity_attr_with_params request failed");
        Self::send_response(resp).await
    }

    pub async fn replace_entity_attr(&self, id: &str, attr_id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.put(self.url(&format!("/entities/{id}/attrs/{attr_id}"))), body).await
    }

    // ── Batch Operations ─────────────────────────────────────────────

    pub async fn batch_create(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entityOperations/create")), body).await
    }

    pub async fn batch_upsert(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entityOperations/upsert")), body).await
    }

    pub async fn batch_update(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entityOperations/update")), body).await
    }

    pub async fn batch_merge(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entityOperations/merge")), body).await
    }

    pub async fn batch_delete(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/entityOperations/delete")), body).await
    }

    pub async fn batch_query(&self, body: &serde_json::Value, headers: RequestHeaders) -> NgsiLdResponse {
        let ct = content_type_for(body);
        let mut req = self.inner.post(self.url("/entityOperations/query"));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req
            .header("Content-Type", ct)
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .expect("batch_query request failed");
        Self::send_response(resp).await
    }

    // ── Temporal Entities ────────────────────────────────────────────

    pub async fn create_temporal_entity(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/temporal/entities")), body).await
    }

    pub async fn get_temporal_entity(&self, id: &str, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params(&format!("/temporal/entities/{id}"), params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("get_temporal_entity request failed");
        Self::send_response(resp).await
    }

    pub async fn query_temporal_entities(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/temporal/entities", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("query_temporal_entities request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_temporal_entity(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/temporal/entities/{id}")))
            .send()
            .await
            .expect("delete_temporal_entity request failed");
        Self::send_response(resp).await
    }

    pub async fn add_temporal_attrs(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url(&format!("/temporal/entities/{id}/attrs"))), body).await
    }

    pub async fn delete_temporal_attr(&self, id: &str, attr_id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/temporal/entities/{id}/attrs/{attr_id}")))
            .send()
            .await
            .expect("delete_temporal_attr request failed");
        Self::send_response(resp).await
    }

    pub async fn modify_temporal_attr_instance(&self, id: &str, attr_id: &str, instance_id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(
            self.inner.patch(self.url(&format!("/temporal/entities/{id}/attrs/{attr_id}/{instance_id}"))),
            body,
        )
        .await
    }

    pub async fn delete_temporal_attr_instance(&self, id: &str, attr_id: &str, instance_id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/temporal/entities/{id}/attrs/{attr_id}/{instance_id}")))
            .send()
            .await
            .expect("delete_temporal_attr_instance request failed");
        Self::send_response(resp).await
    }

    // ── Subscriptions ────────────────────────────────────────────────

    pub async fn create_subscription(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/subscriptions")), body).await
    }

    pub async fn delete_subscription(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/subscriptions/{id}")))
            .send()
            .await
            .expect("delete_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn retrieve_subscription(&self, id: &str, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url(&format!("/subscriptions/{id}")));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        let resp = req.send().await.expect("retrieve_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn update_subscription(&self, id: &str, body: &serde_json::Value, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.patch(self.url(&format!("/subscriptions/{id}")));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        req = req.header("Content-Type", "application/json").body(serde_json::to_vec(body).unwrap());
        let resp = req.send().await.expect("update_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn query_subscriptions(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/subscriptions", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("query_subscriptions request failed");
        Self::send_response(resp).await
    }

    // ── Context Source Registration Subscriptions ─────────────────────

    pub async fn create_csource_subscription(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/csourceSubscriptions")), body).await
    }

    pub async fn retrieve_csource_subscription(&self, id: &str, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url(&format!("/csourceSubscriptions/{id}")));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        let resp = req.send().await.expect("retrieve_csource_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn update_csource_subscription(&self, id: &str, body: &serde_json::Value, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.patch(self.url(&format!("/csourceSubscriptions/{id}")));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        req = req.header("Content-Type", "application/json").body(serde_json::to_vec(body).unwrap());
        let resp = req.send().await.expect("update_csource_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_csource_subscription(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/csourceSubscriptions/{id}")))
            .send()
            .await
            .expect("delete_csource_subscription request failed");
        Self::send_response(resp).await
    }

    pub async fn query_csource_subscriptions(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/csourceSubscriptions", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("query_csource_subscriptions request failed");
        Self::send_response(resp).await
    }

    // ── Context Source Registrations ─────────────────────────────────

    pub async fn create_registration(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/csourceRegistrations")), body).await
    }

    pub async fn delete_registration(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/csourceRegistrations/{id}")))
            .send()
            .await
            .expect("delete_registration request failed");
        Self::send_response(resp).await
    }

    pub async fn retrieve_registration(&self, id: &str, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url(&format!("/csourceRegistrations/{id}")));
        if !headers.is_empty() {
            req = req.headers(headers.into_header_map());
        }
        let resp = req.send().await.expect("retrieve_registration request failed");
        Self::send_response(resp).await
    }

    pub async fn query_registrations(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/csourceRegistrations", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("query_registrations request failed");
        Self::send_response(resp).await
    }

    // ── JSON-LD Contexts ─────────────────────────────────────────────

    pub async fn create_context(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/jsonldContexts")), body).await
    }

    pub async fn get_context(&self, id: &str, params: &ContextQueryParams) -> NgsiLdResponse {
        let qs = params.to_query_string();
        let encoded_id = urlencoding::encode(id);
        let resp = self
            .inner
            .get(format!("{}/ngsi-ld/v1/jsonldContexts/{encoded_id}{qs}", self.base_url))
            .send()
            .await
            .expect("get_context request failed");
        Self::send_response(resp).await
    }

    pub async fn list_contexts_with_params(&self, params: &ContextQueryParams) -> NgsiLdResponse {
        let qs = params.to_query_string();
        let resp = self
            .inner
            .get(format!("{}/ngsi-ld/v1/jsonldContexts{qs}", self.base_url))
            .send()
            .await
            .expect("list_contexts request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_context(&self, id: &str) -> NgsiLdResponse {
        self.delete_context_with_params(id, &ContextQueryParams::new()).await
    }

    pub async fn delete_context_with_params(&self, id: &str, params: &ContextQueryParams) -> NgsiLdResponse {
        let qs = params.to_query_string();
        let encoded_id = urlencoding::encode(id);
        let resp = self
            .inner
            .delete(format!("{}/ngsi-ld/v1/jsonldContexts/{encoded_id}{qs}", self.base_url))
            .send()
            .await
            .expect("delete_context request failed");
        Self::send_response(resp).await
    }

    // ── Entity Maps ──────────────────────────────────────────────────

    pub async fn create_entity_map(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/entityMaps", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("create_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn create_entity_map_post(&self, body: &serde_json::Value, headers: RequestHeaders) -> NgsiLdResponse {
        let ct = content_type_for(body);
        let mut req = self.inner.post(self.url("/entityMaps"));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req
            .header("Content-Type", ct)
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .expect("create_entity_map_post request failed");
        Self::send_response(resp).await
    }

    pub async fn get_entity_map(&self, id: &str, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url(&format!("/entityMaps/{id}")));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("get_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn update_entity_map(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        let resp = self
            .inner
            .patch(self.url(&format!("/entityMaps/{id}")))
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .expect("update_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn delete_entity_map(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/entityMaps/{id}")))
            .send()
            .await
            .expect("delete_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn create_temporal_entity_map(&self, params: &QueryParams, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url_with_params("/temporal/entityMaps", params));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("create_temporal_entity_map request failed");
        Self::send_response(resp).await
    }

    pub async fn create_temporal_entity_map_post(&self, body: &serde_json::Value, headers: RequestHeaders) -> NgsiLdResponse {
        let ct = content_type_for(body);
        let mut req = self.inner.post(self.url("/temporal/entityMaps"));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req
            .header("Content-Type", ct)
            .body(serde_json::to_vec(body).unwrap())
            .send()
            .await
            .expect("create_temporal_entity_map_post request failed");
        Self::send_response(resp).await
    }

    // ── Types & Attributes Discovery ─────────────────────────────────

    pub async fn list_types(&self, params: &QueryParams) -> NgsiLdResponse {
        let resp = self
            .inner
            .get(self.url_with_params("/types", params))
            .send()
            .await
            .expect("list_types request failed");
        Self::send_response(resp).await
    }

    pub async fn get_type(&self, type_name: &str, params: &QueryParams) -> NgsiLdResponse {
        let resp = self
            .inner
            .get(self.url_with_params(&format!("/types/{type_name}"), params))
            .send()
            .await
            .expect("get_type request failed");
        Self::send_response(resp).await
    }

    pub async fn list_attributes(&self, params: &QueryParams) -> NgsiLdResponse {
        let resp = self
            .inner
            .get(self.url_with_params("/attributes", params))
            .send()
            .await
            .expect("list_attributes request failed");
        Self::send_response(resp).await
    }

    pub async fn get_attribute(&self, attr_name: &str, params: &QueryParams) -> NgsiLdResponse {
        let resp = self
            .inner
            .get(self.url_with_params(&format!("/attributes/{attr_name}"), params))
            .send()
            .await
            .expect("get_attribute request failed");
        Self::send_response(resp).await
    }

    // ── Info ─────────────────────────────────────────────────────────

    pub async fn get_source_identity(&self, headers: RequestHeaders) -> NgsiLdResponse {
        let mut req = self.inner.get(self.url("/info/sourceIdentity"));
        let hm = headers.into_header_map();
        if !hm.is_empty() {
            req = req.headers(hm);
        }
        let resp = req.send().await.expect("get_source_identity request failed");
        Self::send_response(resp).await
    }

    // ── Snapshots ────────────────────────────────────────────────────

    pub async fn create_snapshot(&self, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.post(self.url("/snapshots")), body).await
    }

    pub async fn get_snapshot(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .get(self.url(&format!("/snapshots/{id}")))
            .send()
            .await
            .expect("get_snapshot request failed");
        Self::send_response(resp).await
    }

    pub async fn update_snapshot(&self, id: &str, body: &serde_json::Value) -> NgsiLdResponse {
        self.send_json(self.inner.patch(self.url(&format!("/snapshots/{id}"))), body).await
    }

    pub async fn delete_snapshot(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .delete(self.url(&format!("/snapshots/{id}")))
            .send()
            .await
            .expect("delete_snapshot request failed");
        Self::send_response(resp).await
    }

    pub async fn purge_snapshots(&self) -> NgsiLdResponse {
        let resp = self.inner.delete(self.url("/snapshots")).send().await.expect("purge_snapshots request failed");
        Self::send_response(resp).await
    }

    pub async fn clone_snapshot(&self, id: &str) -> NgsiLdResponse {
        let resp = self
            .inner
            .post(self.url(&format!("/snapshots/{id}/clone")))
            .send()
            .await
            .expect("clone_snapshot request failed");
        Self::send_response(resp).await
    }
}
