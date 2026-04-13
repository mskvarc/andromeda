#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{client::QueryParams, fixtures, trial::trial};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "provision::entities::delete_by_query";

pub fn tests(trials: &mut Vec<Trial>) {
    // Delete entities by type
    trials.push(trial(&format!("{PREFIX}::delete_by_type"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:dq_type_1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:dq_type_2");
        let id3 = ctx.entity_id("urn:ngsi-ld:Building:dq_type_3");
        let id_vehicle = ctx.entity_id("urn:ngsi-ld:Vehicle:dq_type_v");

        for id in [&id1, &id2, &id3] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let vehicle = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id_vehicle);
        let resp = ctx.client.create_entity(&vehicle).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").local(true);
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete by type should return 204");

        // Verify buildings are gone
        for id in [&id1, &id2, &id3] {
            let resp = ctx.client.get_entity(id, &QueryParams::new(), Default::default()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "building {id} should be deleted");
        }

        // Verify vehicle remains
        let resp = ctx.client.get_entity(&id_vehicle, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK, "vehicle should still exist");
    }));

    // Delete entities by ID
    trials.push(trial(&format!("{PREFIX}::delete_by_id"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:dq_id_1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:dq_id_2");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let ids = format!("{id1},{id2}");
        let params = QueryParams::new().id(ids).local(true);
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete by id should return 204");

        for id in [&id1, &id2] {
            let resp = ctx.client.get_entity(id, &QueryParams::new(), Default::default()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "entity {id} should be deleted");
        }
    }));

    // Delete entities by idPattern
    trials.push(trial(&format!("{PREFIX}::delete_by_id_pattern"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:dq_pat_match1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:dq_pat_match2");
        let id_other = ctx.entity_id("urn:ngsi-ld:Building:dq_pat_other");

        for id in [&id1, &id2, &id_other] {
            let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        }

        let params = QueryParams::new().id_pattern("urn:ngsi-ld:Building:dq_pat_match.*").local(true);
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete by idPattern should return 204");

        for id in [&id1, &id2] {
            let resp = ctx.client.get_entity(id, &QueryParams::new(), Default::default()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "matched entity {id} should be deleted");
        }

        let resp = ctx.client.get_entity(&id_other, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK, "unmatched entity should remain");
    }));

    // Delete entities by q filter
    trials.push(trial(&format!("{PREFIX}::delete_by_q_filter"), |ctx| async move {
        let id_high = ctx.entity_id("urn:ngsi-ld:Building:dq_q_high");
        let id_low = ctx.entity_id("urn:ngsi-ld:Building:dq_q_low");

        let mut entity_high = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id_high);
        entity_high["airQualityLevel"]["value"] = serde_json::json!(50);
        let resp = ctx.client.create_entity(&entity_high).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let mut entity_low = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id_low);
        entity_low["airQualityLevel"]["value"] = serde_json::json!(2);
        let resp = ctx.client.create_entity(&entity_low).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").q("airQualityLevel>30").local(true);
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "delete by q filter should return 204");

        let resp = ctx.client.get_entity(&id_high, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND, "high value entity should be deleted");

        let resp = ctx.client.get_entity(&id_low, &QueryParams::new(), Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK, "low value entity should remain");
    }));

    // Empty result returns 204
    trials.push(trial(&format!("{PREFIX}::empty_result_returns_204"), |ctx| async move {
        let params = QueryParams::new().type_("NonExistentType").local(true);
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT, "empty result should return 204");
    }));

    // No filters returns 400
    trials.push(trial(&format!("{PREFIX}::no_filters_returns_400"), |ctx| async move {
        let params = QueryParams::new();
        let resp = ctx.client.delete_entities_by_query(&params, Default::default()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST, "no filters should return 400");
    }));
}
