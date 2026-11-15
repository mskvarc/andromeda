//! A csf selects the same registrations in a discovery query and a subscription to registrations.

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

/// Pins: over registrations r1 (`tier` gold, `zones` the array a and b), r2 (`tier` silver), r3 (no added property)
/// and r4 (`meta` with the sub-property `owner` x), Query Context Source Registrations and a Context Source
/// Registration Subscription with each csf select alike: tier=="gold" r1, tier!="gold" r2, tier r1 and r2,
/// zones=="a" r1, zones!="a" none, zones!="c" r1, tier~="g.*" r1, meta.owner=="x" r4, meta.owner r4 and
/// meta.missing none; an array value includes each element, and an unequal term requires the target element
/// Clause: 4.9, 5.2.9, 5.10.2.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9092_a_csf_selects_the_same_registrations_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let ids = [
        namespace.registration_id("r1")?,
        namespace.registration_id("r2")?,
        namespace.registration_id("r3")?,
        namespace.registration_id("r4")?,
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
    let gold: RawJson = r#"{"type": "Property", "value": "gold"}"#.parse()?;
    let zones: RawJson = r#"{"type": "Property", "value": ["a", "b"]}"#.parse()?;
    let silver: RawJson = r#"{"type": "Property", "value": "silver"}"#.parse()?;
    let meta: RawJson = r#"{"type": "Property", "value": "m", "owner": {"type": "Property", "value": "x"}}"#.parse()?;
    let [r1, r2, _, r4] = &ids;
    let [first, second, third, fourth] = registrations.as_slice() else {
        return fail!("four registrations");
    };
    let corpus = [
        CsfRegistration {
            registration: first,
            properties: &[("tier", &gold), ("zones", &zones)],
        },
        CsfRegistration {
            registration: second,
            properties: &[("tier", &silver)],
        },
        CsfRegistration {
            registration: third,
            properties: &[],
        },
        CsfRegistration {
            registration: fourth,
            properties: &[("meta", &meta)],
        },
    ];
    let cases = [
        CsfCase {
            csf: "tier==\"gold\"",
            selects: &[r1],
        },
        CsfCase {
            csf: "tier!=\"gold\"",
            selects: &[r2],
        },
        CsfCase {
            csf: "tier",
            selects: &[r1, r2],
        },
        CsfCase {
            csf: "zones==\"a\"",
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
            csf: "tier~=\"g.*\"",
            selects: &[r1],
        },
        CsfCase {
            csf: "meta.owner==\"x\"",
            selects: &[r4],
        },
        CsfCase {
            csf: "meta.owner",
            selects: &[r4],
        },
        CsfCase {
            csf: "meta.missing",
            selects: &[],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
