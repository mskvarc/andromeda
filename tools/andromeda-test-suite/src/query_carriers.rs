//! The four carriers a `q` travels in, for a test of a limit the broker puts on the query: Query Entities by GET,
//! Query Entities by POST, Query Temporal Evolution of Entities and Create Subscription. A `q` the models would refuse
//! to write is sent as written through each.

use crate::{
    body_json::{json_contains_text, json_equals},
    broker::BrokerClient,
    problem::problem,
    raw_call::{call, target},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::SubscriptionId,
};
use serde_json::json;
use url::Url;

/// Records a failed expectation unless `response`, read as received, is 403 with a `TooComplexQuery` problem (clause
/// 6.3.2) whose `status` member repeats the status and whose `detail` contains `detail`.
fn expect_too_complex(response: &Response<Bytes>, detail: &str) {
    expect_that!(response.status(), eq(StatusCode::FORBIDDEN));
    expect_that!(response.body(), json_equals("$.type", ErrorType::TooComplexQuery.to_string()));
    expect_that!(response.body(), json_equals("$.status", StatusCode::FORBIDDEN.as_u16()));
    expect_that!(response.body(), json_contains_text("$.detail", detail));
}

/// The type queried and the `q` sent through every carrier.
#[derive(Clone, Copy)]
pub struct Carriers<'a> {
    /// The broker queried.
    pub broker: &'a BrokerClient,
    /// The entity type the queries select.
    pub entity_type: &'a str,
    /// The `q` text.
    pub q: &'a str,
}

impl Carriers<'_> {
    /// The Create Subscription body with the identifier `id` and the notification endpoint `endpoint`.
    fn subscription(&self, id: &SubscriptionId, endpoint: &Url) -> String {
        json!({
            "id": id.to_string(),
            "type": "Subscription",
            "entities": [{"type": self.entity_type}],
            "q": self.q,
            "notification": {"endpoint": {"uri": endpoint.as_str(), "accept": "application/json"}}
        })
        .to_string()
    }

    /// Requires Query Entities by GET and Query Temporal Evolution of Entities to answer 200, and Create Subscription
    /// to answer 201.
    ///
    /// # Errors
    ///
    /// Returns the error of a request that cannot be sent.
    pub async fn expect_served(&self, id: &SubscriptionId, endpoint: &Url) -> Result<()> {
        let json = [("Accept", "application/json")];
        let entities = call(
            self.broker,
            Method::GET,
            &target("entities", &[("type", self.entity_type), ("q", self.q)]),
            &json,
            None,
        )
        .await?;
        expect_that!(entities.status(), eq(StatusCode::OK));
        let temporal = call(
            self.broker,
            Method::GET,
            &target(
                "temporal/entities",
                &[
                    ("type", self.entity_type),
                    ("timerel", "after"),
                    ("timeAt", "2020-01-01T00:00:00Z"),
                    ("q", self.q),
                ],
            ),
            &json,
            None,
        )
        .await?;
        expect_that!(temporal.status(), eq(StatusCode::OK));
        let created = call(
            self.broker,
            Method::POST,
            "subscriptions",
            &[("Content-Type", "application/json")],
            Some(self.subscription(id, endpoint).as_str()),
        )
        .await?;
        expect_that!(created.status(), eq(StatusCode::CREATED));
        Ok(())
    }

    /// Requires every carrier to answer 403 `TooComplexQuery` with a `detail` containing `detail`, and the refused
    /// subscription `id` not to exist.
    ///
    /// # Errors
    ///
    /// Returns the error of a request that cannot be sent.
    pub async fn expect_refused(&self, id: &SubscriptionId, endpoint: &Url, detail: &str) -> Result<()> {
        let json = [("Accept", "application/json")];
        let entities = call(
            self.broker,
            Method::GET,
            &target("entities", &[("type", self.entity_type), ("q", self.q)]),
            &json,
            None,
        )
        .await?;
        expect_too_complex(&entities, detail);
        let query = json!({"type": "Query", "entities": [{"type": self.entity_type}], "q": self.q}).to_string();
        let posted = call(
            self.broker,
            Method::POST,
            "entityOperations/query",
            &[("Content-Type", "application/json"), ("Accept", "application/json")],
            Some(&query),
        )
        .await?;
        expect_too_complex(&posted, detail);
        let temporal = call(
            self.broker,
            Method::GET,
            &target(
                "temporal/entities",
                &[
                    ("type", self.entity_type),
                    ("timerel", "after"),
                    ("timeAt", "2020-01-01T00:00:00Z"),
                    ("q", self.q),
                ],
            ),
            &json,
            None,
        )
        .await?;
        expect_too_complex(&temporal, detail);
        let created = call(
            self.broker,
            Method::POST,
            "subscriptions",
            &[("Content-Type", "application/json")],
            Some(self.subscription(id, endpoint).as_str()),
        )
        .await?;
        expect_too_complex(&created, detail);
        let absent = self
            .broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&absent, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
        Ok(())
    }
}
