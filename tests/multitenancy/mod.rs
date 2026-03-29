use crate::harness::{client::RequestHeaders, fixtures, notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use std::time::Duration;

const PREFIX: &str = "multitenancy";

fn core_context() -> &'static str {
    fixtures::test_suite_context_url()
}

fn sample_entity(id: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "Vehicle",
        "name": {
            "type": "Property",
            "value": name
        },
        "@context": core_context()
    })
}

fn sample_temporal_entity(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "values": [
                {
                    "value": 55,
                    "observedAt": "2020-08-01T12:00:00Z"
                }
            ]
        },
        "@context": core_context()
    })
}

fn sample_subscription(id: &str, endpoint: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {
                "uri": endpoint,
                "accept": "application/json"
            }
        },
        "@context": core_context()
    })
}

fn sample_csr_subscription(id: &str, endpoint: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {
            "endpoint": {
                "uri": endpoint,
                "accept": "application/json"
            }
        },
        "@context": core_context()
    })
}

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::unnamed_vs_named_default_isolation"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:tenant-isolation:{}", uuid::Uuid::new_v4());
        let unnamed_entity = sample_entity(&id, "unnamed-default");
        let named_entity = sample_entity(&id, "named-default");

        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&unnamed_entity), RequestHeaders::new())
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert!(
            resp.headers.get("NGSILD-Tenant").is_none(),
            "unnamed default response must not include NGSILD-Tenant"
        );

        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&named_entity), RequestHeaders::new().tenant("default"))
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some("default"));

        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}"), None, RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        assert_eq!(body["name"]["value"], "unnamed-default");

        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/entities/{id}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant("default"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body = resp.json_value();
        assert_eq!(body["name"]["value"], "named-default");
    }));

    trials.push(trial(&format!("{PREFIX}::non_create_unknown_tenant_is_nonexistent_tenant"), |ctx| async move {
        let tenant = format!("unknown-tenant-{}", uuid::Uuid::new_v4());
        let id = format!("urn:ngsi-ld:Vehicle:noncreate:{}", uuid::Uuid::new_v4());

        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/entities/{id}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body = resp.json_value();
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant",
            "unknown tenant on non-create must return NonexistentTenant"
        );
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant.as_str()));
    }));

    trials.push(trial(&format!("{PREFIX}::implicit_creation_matrix_for_create_operations"), |ctx| async move {
        // 1) Create Entity
        let tenant_1 = format!("tenant-create-entity-{}", uuid::Uuid::new_v4());
        let id_1 = format!("urn:ngsi-ld:Vehicle:create-entity:{}", uuid::Uuid::new_v4());
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities",
                Some(&sample_entity(&id_1, "created-via-create-entity")),
                RequestHeaders::new().tenant(&tenant_1),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_1.as_str()));
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/entities/{id_1}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_1),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id_1}"), None, RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        // 2) Batch Entity Creation
        let tenant_2 = format!("tenant-batch-create-{}", uuid::Uuid::new_v4());
        let id_2 = format!("urn:ngsi-ld:Vehicle:batch-create:{}", uuid::Uuid::new_v4());
        let batch = serde_json::json!([sample_entity(&id_2, "created-via-batch-create")]);
        let resp = ctx
            .client
            .request(Method::POST, "/entityOperations/create", Some(&batch), RequestHeaders::new().tenant(&tenant_2))
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_2.as_str()));
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/entities/{id_2}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_2),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id_2}"), None, RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        // 3) Create/Update Temporal Evolution
        let tenant_3 = format!("tenant-temporal-create-{}", uuid::Uuid::new_v4());
        let id_3 = format!("urn:ngsi-ld:Vehicle:temporal-create:{}", uuid::Uuid::new_v4());
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/temporal/entities",
                Some(&sample_temporal_entity(&id_3)),
                RequestHeaders::new().tenant(&tenant_3),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_3.as_str()));

        // 4) Create Subscription
        let tenant_4 = format!("tenant-sub-create-{}", uuid::Uuid::new_v4());
        let id_4 = format!("urn:ngsi-ld:Subscription:create:{}", uuid::Uuid::new_v4());
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/subscriptions",
                Some(&sample_subscription(&id_4, "http://127.0.0.1:65535/notify")),
                RequestHeaders::new().tenant(&tenant_4),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_4.as_str()));
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/subscriptions/{id_4}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_4),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx
            .client
            .request(Method::GET, &format!("/subscriptions/{id_4}"), None, RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        // 5) Register Context Source
        let tenant_5 = format!("tenant-csr-create-{}", uuid::Uuid::new_v4());
        let id_5 = format!("urn:ngsi-ld:ContextSourceRegistration:create:{}", uuid::Uuid::new_v4());
        let reg = fixtures::load_fixture_original_context("csource_registrations/context-source-registration.jsonld", &id_5);
        let resp = ctx
            .client
            .request(Method::POST, "/csourceRegistrations", Some(&reg), RequestHeaders::new().tenant(&tenant_5))
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_5.as_str()));
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/csourceRegistrations/{id_5}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_5),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/csourceRegistrations/{id_5}"),
                None,
                RequestHeaders::new().accept_ld_json(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);

        // 6) Create Context Source Subscription
        let tenant_6 = format!("tenant-csr-sub-create-{}", uuid::Uuid::new_v4());
        let id_6 = format!("urn:ngsi-ld:Subscription:csource-create:{}", uuid::Uuid::new_v4());
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/csourceSubscriptions",
                Some(&sample_csr_subscription(&id_6, "http://127.0.0.1:65535/notify")),
                RequestHeaders::new().tenant(&tenant_6),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant_6.as_str()));
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/csourceSubscriptions/{id_6}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_6),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/csourceSubscriptions/{id_6}"),
                None,
                RequestHeaders::new().accept_ld_json(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    trials.push(trial(&format!("{PREFIX}::named_tenant_error_response_echoes_header"), |ctx| async move {
        let tenant = format!("tenant-error-echo-{}", uuid::Uuid::new_v4());
        let invalid = serde_json::json!({"type": "Vehicle", "@context": core_context()});

        let resp = ctx
            .client
            .request(Method::POST, "/entities", Some(&invalid), RequestHeaders::new().tenant(&tenant))
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant.as_str()));
    }));

    trials.push(trial(&format!("{PREFIX}::named_tenant_cross_visibility_isolated"), |ctx| async move {
        let id = format!("urn:ngsi-ld:Vehicle:cross-visibility:{}", uuid::Uuid::new_v4());
        let tenant_a = format!("tenant-a-{}", uuid::Uuid::new_v4());
        let tenant_b = format!("tenant-b-{}", uuid::Uuid::new_v4());

        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities",
                Some(&sample_entity(&id, "tenant-a")),
                RequestHeaders::new().tenant(&tenant_a),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(
                Method::GET,
                &format!("/entities/{id}"),
                None,
                RequestHeaders::new().accept_ld_json().tenant(&tenant_b),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let body = resp.json_value();
        assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/NonexistentTenant");
    }));

    trials.push(trial(
        &format!("{PREFIX}::unnamed_default_notification_omits_tenant_header"),
        |ctx| async move {
            let mut receiver = NotificationReceiver::start().await;

            let sub_id = format!("urn:ngsi-ld:Subscription:unnamed-notif:{}", uuid::Uuid::new_v4());
            let sub = sample_subscription(&sub_id, &receiver.endpoint_url());
            let resp = ctx.client.request(Method::POST, "/subscriptions", Some(&sub), RequestHeaders::new()).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            assert!(resp.headers.get("NGSILD-Tenant").is_none(), "unnamed default response must omit NGSILD-Tenant");

            let entity_id = format!("urn:ngsi-ld:Vehicle:unnamed-notif:{}", uuid::Uuid::new_v4());
            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/entities",
                    Some(&sample_entity(&entity_id, "unnamed-notification-trigger")),
                    RequestHeaders::new(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let notif = receiver.wait_for_notification(Duration::from_secs(10)).await;
            assert!(
                notif.headers.get("NGSILD-Tenant").is_none(),
                "unnamed default notifications must omit NGSILD-Tenant"
            );
            receiver.stop();
        },
    ));

    trials.push(trial(&format!("{PREFIX}::notification_includes_tenant_header"), |ctx| async move {
        let tenant = format!("tenant-notification-{}", uuid::Uuid::new_v4());
        let mut receiver = NotificationReceiver::start().await;

        let sub_id = format!("urn:ngsi-ld:Subscription:notif:{}", uuid::Uuid::new_v4());
        let sub = sample_subscription(&sub_id, &receiver.endpoint_url());
        let resp = ctx
            .client
            .request(Method::POST, "/subscriptions", Some(&sub), RequestHeaders::new().tenant(&tenant))
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        assert_eq!(resp.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()), Some(tenant.as_str()));

        let entity_id = format!("urn:ngsi-ld:Vehicle:notif:{}", uuid::Uuid::new_v4());
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities",
                Some(&sample_entity(&entity_id, "notification-trigger")),
                RequestHeaders::new().tenant(&tenant),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let notif = receiver.wait_for_notification(Duration::from_secs(10)).await;
        assert_eq!(
            notif.headers.get("NGSILD-Tenant").and_then(|v| v.to_str().ok()),
            Some(tenant.as_str()),
            "notification must include tenant header for named tenants"
        );
        receiver.stop();
    }));
}
