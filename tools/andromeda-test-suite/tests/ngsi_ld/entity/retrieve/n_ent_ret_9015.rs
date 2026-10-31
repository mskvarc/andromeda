//! A json array reads back as written in every representation.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_exists},
    broker,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity, Simplified},
    },
    model::{Entity, JsonProperty, Lossless, NgsiLdMediaType, Property, RawJson},
};

/// Pins: Retrieve Entity of `JsonProperties` holding a one-element array and an array holding an array, and of a
/// sub-attribute `JsonProperty` holding a one-element array, answers each json as written in the normalized,
/// concise and `keyValues` representations
/// Clause: 4.5.24.2, 4.5.24.3, 4.6.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_9015_a_json_array_reads_back_as_written_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let json_array_probe_read_id = namespace.urn("JsonArrayProbe", "read")?;
    let json_array_probe_type = namespace.entity_type("JsonArrayProbe")?;
    let json_array_probe_read = Entity::builder()
        .id(json_array_probe_read_id.clone())
        .entity_type(json_array_probe_type)
        .json_property(
            "tickets",
            JsonProperty::builder(r#"[{"code": "85a6cc52-0589-45f9", "value": "Overstay 60 minutes"}]"#.parse::<RawJson>()?),
        )
        .json_property(
            "nested",
            JsonProperty::builder(r#"[{"b": 2, "a": 1}, {"z": [1, 2], "y": "text"}]"#.parse::<RawJson>()?),
        )
        .property(
            "speed",
            Property::builder(1).json_property("meta", JsonProperty::builder(r#"[{"k": "v"}]"#.parse::<RawJson>()?)),
        )
        .build()?;
    broker.entities().create(&json_array_probe_read).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let in_options = RepresentationSpelling::default().in_options();
    // Each representation is read as received: the test pins the members every form writes beside the json, which
    // the model reads into one shape.
    let retrieved = json
        .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(&json_array_probe_read_id).format(Lossless::Normalized).build())?)
        .await?;
    let body = retrieved.body();
    expect_that!(body, json_exists("$[?count($.tickets.*)==2]"));
    expect_that!(body, json_equals("$.tickets.type", "JsonProperty"));
    expect_that!(body, json_count("$.tickets.json", 1));
    expect_that!(body, json_exists("$[?count($.tickets.json[0].*)==2]"));
    expect_that!(body, json_equals("$.tickets.json[0].code", "85a6cc52-0589-45f9"));
    expect_that!(body, json_equals("$.tickets.json[0].value", "Overstay 60 minutes"));
    expect_that!(body, json_exists("$[?count($.nested.*)==2]"));
    expect_that!(body, json_count("$.nested.json", 2));
    expect_that!(body, json_exists("$[?count($.nested.json[0].*)==2]"));
    expect_that!(body, json_equals("$.nested.json[0].b", 2));
    expect_that!(body, json_equals("$.nested.json[0].a", 1));
    expect_that!(body, json_exists("$[?count($.nested.json[1].*)==2]"));
    expect_that!(body, json_count("$.nested.json[1].z", 2));
    expect_that!(body, json_equals("$.nested.json[1].z[0]", 1));
    expect_that!(body, json_equals("$.nested.json[1].z[1]", 2));
    expect_that!(body, json_equals("$.nested.json[1].y", "text"));
    expect_that!(body, json_exists("$[?count($.speed.meta.*)==2]"));
    expect_that!(body, json_equals("$.speed.meta.type", "JsonProperty"));
    expect_that!(body, json_count("$.speed.meta.json", 1));
    expect_that!(body, json_exists("$[?count($.speed.meta.json[0].*)==1]"));
    expect_that!(body, json_equals("$.speed.meta.json[0].k", "v"));
    let retrieved_2 = json
        .send_unchecked(
            json.unchecked(
                &RetrieveEntity::builder()
                    .id(&json_array_probe_read_id)
                    .format(Lossless::Concise)
                    .spelling(in_options)
                    .build(),
            )?,
        )
        .await?;
    let body_2 = retrieved_2.body();
    expect_that!(body_2, json_exists("$[?count($.tickets.*)==1]"));
    expect_that!(body_2, json_count("$.tickets.json", 1));
    expect_that!(body_2, json_exists("$[?count($.tickets.json[0].*)==2]"));
    expect_that!(body_2, json_equals("$.tickets.json[0].code", "85a6cc52-0589-45f9"));
    expect_that!(body_2, json_equals("$.tickets.json[0].value", "Overstay 60 minutes"));
    expect_that!(body_2, json_exists("$[?count($.nested.*)==1]"));
    expect_that!(body_2, json_count("$.nested.json", 2));
    expect_that!(body_2, json_exists("$[?count($.nested.json[0].*)==2]"));
    expect_that!(body_2, json_equals("$.nested.json[0].b", 2));
    expect_that!(body_2, json_equals("$.nested.json[0].a", 1));
    expect_that!(body_2, json_exists("$[?count($.nested.json[1].*)==2]"));
    expect_that!(body_2, json_count("$.nested.json[1].z", 2));
    expect_that!(body_2, json_equals("$.nested.json[1].z[0]", 1));
    expect_that!(body_2, json_equals("$.nested.json[1].z[1]", 2));
    expect_that!(body_2, json_equals("$.nested.json[1].y", "text"));
    let retrieved_3 = json
        .send_unchecked(
            json.unchecked(
                &RetrieveEntity::builder()
                    .id(&json_array_probe_read_id)
                    .format(Simplified)
                    .spelling(in_options.key_values())
                    .build(),
            )?,
        )
        .await?;
    let body_3 = retrieved_3.body();
    expect_that!(body_3, json_exists("$[?count($.tickets.*)==1]"));
    expect_that!(body_3, json_count("$.tickets.json", 1));
    expect_that!(body_3, json_exists("$[?count($.tickets.json[0].*)==2]"));
    expect_that!(body_3, json_equals("$.tickets.json[0].code", "85a6cc52-0589-45f9"));
    expect_that!(body_3, json_equals("$.tickets.json[0].value", "Overstay 60 minutes"));
    expect_that!(body_3, json_exists("$[?count($.nested.*)==1]"));
    expect_that!(body_3, json_count("$.nested.json", 2));
    expect_that!(body_3, json_exists("$[?count($.nested.json[0].*)==2]"));
    expect_that!(body_3, json_equals("$.nested.json[0].b", 2));
    expect_that!(body_3, json_equals("$.nested.json[0].a", 1));
    expect_that!(body_3, json_exists("$[?count($.nested.json[1].*)==2]"));
    expect_that!(body_3, json_count("$.nested.json[1].z", 2));
    expect_that!(body_3, json_equals("$.nested.json[1].z[0]", 1));
    expect_that!(body_3, json_equals("$.nested.json[1].z[1]", 2));
    expect_that!(body_3, json_equals("$.nested.json[1].y", "text"));
    Ok(())
}
