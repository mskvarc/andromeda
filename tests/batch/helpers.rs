#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};

pub fn client() -> Client {
    Client::new()
}

pub fn building_minimal(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_simple_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_simple_attributes_json(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        }
    })
}

pub fn building_relationship(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:City:Paris"
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_relationship_of_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "airQualityLevel": {
            "type": "Property",
            "value": 4,
            "unitCode": "C62",
            "observedAt": "2020-09-09T16:40:00.000Z",
            "hasWeatherObserved": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Sensor:43a1af27-4aa7-44fe-bee9-fe76d56978a4"
            }
        },
        "almostFull": {
            "type": "Property",
            "value": false
        },
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_minimal_with_new_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": ["Building", "Vehicle"],
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_with_one_scope(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte",
        "@context": [CORE_CONTEXT]
    })
}

pub fn building_with_two_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": ["Building", "TouristDestination"],
        "@context": [CORE_CONTEXT]
    })
}

async fn read_json_or_null(response: reqwest::Response) -> Value {
    match response.json::<Value>().await {
        Ok(body) => body,
        Err(_) => Value::Null,
    }
}

pub async fn create_entity(http_client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create entity request failed");

    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn get_entity(http_client: &Client, id: &str, query: &str, with_link_context: bool) -> (StatusCode, HeaderMap, Value) {
    let encoded_id = urlencoding::encode(id);
    let sep = if query.is_empty() { "" } else { "?" };
    let mut request = http_client
        .get(format!("{BASE_URL}/entities/{encoded_id}{sep}{query}"))
        .header("Accept", "application/ld+json");

    if with_link_context {
        request = request.header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }

    let response = request.send().await.expect("get entity request failed");
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_json_or_null(response).await;
    (status, headers, body)
}

pub async fn batch_create(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("batch create request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn batch_update(http_client: &Client, body: &Value, no_overwrite: bool) -> (StatusCode, Value) {
    let url = if no_overwrite {
        format!("{BASE_URL}/entityOperations/update?options=noOverwrite")
    } else {
        format!("{BASE_URL}/entityOperations/update")
    };
    let response = http_client
        .post(url)
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("batch update request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn batch_upsert(http_client: &Client, body: &Value, options_update: bool) -> (StatusCode, Value) {
    let url = if options_update {
        format!("{BASE_URL}/entityOperations/upsert?options=update")
    } else {
        format!("{BASE_URL}/entityOperations/upsert")
    };
    let response = http_client
        .post(url)
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("batch upsert request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn batch_merge(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/entityOperations/merge"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("batch merge request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn batch_delete(http_client: &Client, body: &Value) -> (StatusCode, Value) {
    let response = http_client
        .post(format!("{BASE_URL}/entityOperations/delete"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("batch delete request failed");

    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}

pub async fn request_raw(
    http_client: &Client,
    method: Method,
    path: &str,
    body: &[u8],
    content_type: Option<&str>,
    with_link_context: bool,
) -> (StatusCode, Value) {
    let mut request = http_client.request(method, format!("{BASE_URL}{path}")).body(body.to_vec());

    if let Some(ct) = content_type {
        request = request.header("Content-Type", ct);
    }

    if with_link_context {
        request = request.header(
            "Link",
            format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        );
    }

    let response = request.send().await.expect("raw request failed");
    let status = response.status();
    let body = read_json_or_null(response).await;
    (status, body)
}
