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
/// (`nodes` an array holding the Property node x), r4 (`empty` the empty array), r5 (`nested` the array 1 and the array
/// 2) and r6 (`tier` the Property gold with `unitCode` C62 and `observedAt` 2024-01-01T00:00:00Z, `link` a Relationship
/// node with `object` urn:x:1, `owner` the Property acme with the sub-property `name` Alice), Query Context Source
/// Registrations and a Context Source Registration Subscription with each csf select alike: zones=="b" r1, zones!="a"
/// none, zones!="c" r1, rank==5 r1, rank>2 none, concise=="x" r2, typed r2, typed!="x" none, concise.value=="x" none,
/// typed.type=="Property" none, settings[timeout]==5 r2, settings.timeout==5 none, nodes=="x" none, nodes r3, empty r4,
/// empty!="x" r4, nested==1 r5, nested==2 none, nested!=2 r5, tier.unitCode=="C62" r6, the same member named by its
/// expanded IRI r6, tier.observedAt>2023-12-31T00:00:00Z r6, tier.unitCode=="KGM" none, link.object=="urn:x:1" none and
/// owner.name=="Alice" r6; a Context Source Property is one Property whose bare array is one array value, an object
/// holding `value` or naming its family by `type` is its node, a node holding no value has no target value, an instance
/// member is read under the term clause 4.5.2.2 writes it by, `type` and the members any family defines (core context
/// terms clause 4.5.1 bars as Attribute names) and a key of a value written alone are no sub-property, `name` is one,
/// and an array is compared by its single elements
/// Clause: 4.5.1, 4.5.2.2, 4.5.2.3, 4.9, 5.2.9, 5.10.2.4, 5.11.2.4
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
        namespace.registration_id("r6")?,
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
    let tier: RawJson = r#"{"type": "Property", "value": "gold", "unitCode": "C62", "observedAt": "2024-01-01T00:00:00Z"}"#.parse()?;
    let link: RawJson = r#"{"type": "Relationship", "object": "urn:x:1"}"#.parse()?;
    let owner: RawJson = r#"{"type": "Property", "value": "acme", "name": {"type": "Property", "value": "Alice"}}"#.parse()?;
    let [r1, r2, r3, r4, r5, r6] = &ids;
    let [first, second, third, fourth, fifth, sixth] = registrations.as_slice() else {
        return fail!("six registrations");
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
        CsfRegistration {
            registration: sixth,
            properties: &[("tier", &tier), ("link", &link), ("owner", &owner)],
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
            selects: &[],
        },
        CsfCase {
            csf: "concise.value==\"x\"",
            selects: &[],
        },
        CsfCase {
            csf: "typed.type==\"Property\"",
            selects: &[],
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
        CsfCase {
            csf: "tier.unitCode==\"C62\"",
            selects: &[r6],
        },
        CsfCase {
            csf: "tier.\"https://uri.etsi.org/ngsi-ld/unitCode\"==\"C62\"",
            selects: &[r6],
        },
        CsfCase {
            csf: "tier.observedAt>2023-12-31T00:00:00Z",
            selects: &[r6],
        },
        CsfCase {
            csf: "tier.unitCode==\"KGM\"",
            selects: &[],
        },
        CsfCase {
            csf: "link.object==\"urn:x:1\"",
            selects: &[],
        },
        CsfCase {
            csf: "owner.name==\"Alice\"",
            selects: &[r6],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
