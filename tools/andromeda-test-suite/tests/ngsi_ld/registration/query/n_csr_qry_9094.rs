//! Every term of a grid of `csf` terms reads every written form of a Context Source Property alike in a discovery query
//! and a subscription to registrations.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    csf_parity::{CsfRegistration, expect_registration_query_and_subscription_agree},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::RawJson,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// The written forms of the property `p`, one registration each: scalars, arrays, nodes of several families, nodes
/// carrying instance members and sub-properties, and values that are objects of their own.
const FORMS: [&str; 20] = [
    r#""x""#,
    "1",
    "true",
    r#"["a", "b"]"#,
    "[]",
    "[1, [2]]",
    r#"{"value": "x"}"#,
    r#"{"type": "Property"}"#,
    r#"{"type": "Property", "value": ["a"]}"#,
    r#"{"m": 1}"#,
    r#"{"type": "Property", "value": {"m": 1}}"#,
    r#"{"type": "Relationship", "object": "urn:x:1"}"#,
    r#"{"type": "Property", "value": 1, "unitCode": "C62", "observedAt": "2024-01-01T00:00:00Z"}"#,
    r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00+00:00"}"#,
    r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00,5Z"}"#,
    r#"{"type": "Property", "value": 1, "observedAt": "2024-01-01"}"#,
    r#"{"type": "Property", "value": 1, "name": {"type": "Property", "value": "n"}}"#,
    r#"{"value": "x", "lang": "en"}"#,
    r#"{"@value": "x"}"#,
    r#"{"type": "Point", "coordinates": [1, 2]}"#,
];

/// The terms asked of every form: existence, equality and its complement over each kind, an order, a pattern and its
/// complement, compound members, a family's own members, instance members and a sub-property.
const TERMS: [&str; 26] = [
    "p",
    "p==\"x\"",
    "p!=\"x\"",
    "p==1",
    "p!=1",
    "p>0",
    "p==true",
    "p!=true",
    "p==\"a\"",
    "p!=\"a\"",
    "p~=\"^x\"",
    "p!~=\"^x\"",
    "p[m]",
    "p[m]==1",
    "p[type]==\"Point\"",
    "p.m==1",
    "p.value==\"x\"",
    "p.type==\"Property\"",
    "p.object==\"urn:x:1\"",
    "p.unitCode==\"C62\"",
    "p.unitCode",
    "p.observedAt>2023-12-31T00:00:00Z",
    "p.observedAt==\"2024-01-01\"",
    "p.name==\"n\"",
    "p.name",
    "p.lang==\"en\"",
];

/// Pins: over twenty registrations, each carrying one written form of the property `p` (a String, a number, a Boolean,
/// a bare array, the empty array, a nested array, a concise node, a typed node holding no value, a node holding an
/// array, a bare object, a node holding an object, a Relationship node, a node with `unitCode` and `observedAt`, nodes
/// whose `observedAt` carries an offset, a decimal comma or a bare date, a node with a sub-property `name`, a concise
/// node with `lang`, a JSON-LD value object and a `GeoJSON` geometry), each of twenty-six terms selects in Query
/// Context Source Registrations exactly the registrations its Context Source Registration Subscription is notified
/// of; the two executions of a `csf` read every written form alike, whatever they select
/// Clause: 4.5.1, 4.5.2.2, 4.5.2.3, 4.6.3, 4.9, 5.2.9, 5.10.2.4, 5.11.2.4
/// Tenant: private (the grid owes about a hundred notifications, and a tenant's delivery slots are shared by every
/// Subscription in it, so in the default tenant the slow and failing endpoints of other tests hold the slots the grid's
/// notifications wait on; no namespace narrows that pool)
#[gtest]
#[tokio::test]
async fn n_csr_qry_9094_every_term_reads_every_written_form_alike_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let mut registrations = Vec::with_capacity(FORMS.len());
    let mut values = Vec::with_capacity(FORMS.len());
    for (index, form) in FORMS.iter().enumerate() {
        registrations.push(
            CSourceRegistration::builder(config.unreachable_origin().as_str())
                .id(namespace.registration_id(format!("g{index}"))?)
                .information([RegistrationInfo::builder()
                    .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
                    .build()?])
                .build()?,
        );
        values.push(form.parse::<RawJson>()?);
    }
    let properties: Vec<[(&str, &RawJson); 1]> = values.iter().map(|value| [("p", value)]).collect();
    let corpus: Vec<CsfRegistration<'_>> = registrations
        .iter()
        .zip(&properties)
        .map(|(registration, properties)| CsfRegistration {
            registration,
            properties: properties.as_slice(),
        })
        .collect();
    expect_registration_query_and_subscription_agree(&broker, &namespace, &device_type, &corpus, &TERMS).await
}
