//! A Context Source Property reads alike in a discovery query and a subscription to registrations, in every form a
//! registrant may write it.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    csf_parity::{CsfCase, CsfRegistration, expect_registration_query_and_subscription_alike},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::RawJson,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: over registrations r1 (`zones` the bare array a and b, `rank` the bare array 1 and 5), r2 (`concise` the
/// concise Property x, `typed` a Property node holding no value, `settings` the object value with `timeout` 5), r3
/// (`nodes` an array holding the Property node x), r4 (`empty` the empty array) and r5 (`nested` the array 1 and the
/// array 2), Query Context Source Registrations and a Context Source Registration Subscription with each csf select
/// alike: zones=="b" r1, zones!="a" none, zones!="c" r1, rank==5 r1, rank>2 none, concise=="x" r2, typed r2,
/// typed!="x" r2, settings[timeout]==5 r2, settings.timeout==5 none, nodes=="x" none, nodes r3, empty r4,
/// empty!="x" r4, nested==1 r5, nested==2 none and nested!=2 r5; a Context Source Property is one Property whose
/// bare array is one array value, an object holding `value` or naming its family by `type` is its node, a value
/// written alone has no sub-property, and an array is compared by its single elements
/// Clause: 4.5.2.3, 4.9, 5.2.9, 5.10.2.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9093_a_context_source_property_reads_alike_in_every_written_form() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let ids = [
        namespace.registration_id("r1")?,
        namespace.registration_id("r2")?,
        namespace.registration_id("r3")?,
        namespace.registration_id("r4")?,
        namespace.registration_id("r5")?,
    ];
    let mut registrations = Vec::with_capacity(ids.len());
    for id in &ids {
        registrations.push(
            CSourceRegistration::builder(config.unreachable_origin().as_str())
                .id(id.clone())
                .information([RegistrationInfo::builder()
                    .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
                    .build()?])
                .build()?,
        );
    }
    let zones: RawJson = r#"["a", "b"]"#.parse()?;
    let rank: RawJson = "[1, 5]".parse()?;
    let concise: RawJson = r#"{"value": "x"}"#.parse()?;
    let typed: RawJson = r#"{"type": "Property"}"#.parse()?;
    let settings: RawJson = r#"{"timeout": 5}"#.parse()?;
    let nodes: RawJson = r#"[{"type": "Property", "value": "x"}]"#.parse()?;
    let empty: RawJson = "[]".parse()?;
    let nested: RawJson = "[1, [2]]".parse()?;
    let [r1, r2, r3, r4, r5] = &ids;
    let [first, second, third, fourth, fifth] = registrations.as_slice() else {
        return fail!("five registrations");
    };
    let corpus = [
        CsfRegistration {
            registration: first,
            properties: &[("zones", &zones), ("rank", &rank)],
        },
        CsfRegistration {
            registration: second,
            properties: &[("concise", &concise), ("typed", &typed), ("settings", &settings)],
        },
        CsfRegistration {
            registration: third,
            properties: &[("nodes", &nodes)],
        },
        CsfRegistration {
            registration: fourth,
            properties: &[("empty", &empty)],
        },
        CsfRegistration {
            registration: fifth,
            properties: &[("nested", &nested)],
        },
    ];
    let cases = [
        CsfCase {
            csf: "zones==\"b\"",
            selects: &[r1],
        },
        CsfCase {
            csf: "zones!=\"a\"",
            selects: &[],
        },
        CsfCase {
            csf: "zones!=\"c\"",
            selects: &[r1],
        },
        CsfCase {
            csf: "rank==5",
            selects: &[r1],
        },
        CsfCase { csf: "rank>2", selects: &[] },
        CsfCase {
            csf: "concise==\"x\"",
            selects: &[r2],
        },
        CsfCase { csf: "typed", selects: &[r2] },
        CsfCase {
            csf: "typed!=\"x\"",
            selects: &[r2],
        },
        CsfCase {
            csf: "settings[timeout]==5",
            selects: &[r2],
        },
        CsfCase {
            csf: "settings.timeout==5",
            selects: &[],
        },
        CsfCase {
            csf: "nodes==\"x\"",
            selects: &[],
        },
        CsfCase { csf: "nodes", selects: &[r3] },
        CsfCase { csf: "empty", selects: &[r4] },
        CsfCase {
            csf: "empty!=\"x\"",
            selects: &[r4],
        },
        CsfCase {
            csf: "nested==1",
            selects: &[r5],
        },
        CsfCase {
            csf: "nested==2",
            selects: &[],
        },
        CsfCase {
            csf: "nested!=2",
            selects: &[r5],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
