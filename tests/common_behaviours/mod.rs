pub mod get_without_accept;
pub mod invalid_parameters;
pub mod ld_context_not_available;
pub mod merge_patch_json;
pub mod not_acceptable_media_type;
pub mod unsupported_media_type;

use crate::harness::{client::QueryParams, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;

const PREFIX: &str = "common";

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::smoke::create_retrieve_delete"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:T:smoke");
        let entity = serde_json::json!({
            "id": &id,
            "type": "Vehicle",
            "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
        });

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, reqwest::StatusCode::CREATED, "create should return 201");
        assert!(resp.location().is_some(), "create should return Location header");

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, reqwest::StatusCode::OK, "get should return 200");
        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"], id);
        assert_eq!(body["type"], "Vehicle");

        let resp = ctx.client.delete_entity(&id).await;
        assert_eq!(resp.status, reqwest::StatusCode::NO_CONTENT, "delete should return 204");

        let resp = ctx.client.get_entity(&id, &QueryParams::new(), Default::default()).await;
        assert_eq!(resp.status, reqwest::StatusCode::NOT_FOUND, "get after delete should return 404");
    }));

    invalid_parameters::tests(trials);
    unsupported_media_type::tests(trials);
    not_acceptable_media_type::tests(trials);
    merge_patch_json::tests(trials);
    ld_context_not_available::tests(trials);
    get_without_accept::tests(trials);
}
