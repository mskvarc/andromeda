//! A declared time value of a Context Source Property reads alike in a discovery query and a subscription to
//! registrations, and is a time value only in the clause 4.6.3 form of its datatype.

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

/// Pins: over registrations r1 (`since` declared the `DateTime` 2024-01-01T00:00:00Z, `day` the `Date` 2024-01-01,
/// `opening` a Property node holding the `Time` 08:00:00Z, `comma` the `DateTime` 2024-01-01T00:00:00,5Z), r2 (`since`
/// declared the `DateTime` 2024-01-01T00:00:00+00:00, `day` the `Date` 2024-1-1, `opening` the `Time` 08:00:00+01:00,
/// `calendar` the `Date` 2024-02-30) and r3 (`list` an array holding the declared `DateTime` 2024-01-01T00:00:00Z,
/// `held` an object whose member `m` holds it), Query Context Source Registrations and a Context Source Registration
/// Subscription with each csf select alike: since>2023-12-31T00:00:00Z r1, since!=2024-01-01T00:00:00Z r2,
/// since=="2024-01-01T00:00:00+00:00" none, since==2024-01-01T00:00:00Z,"x" r1, day>2023-12-31 r1, day!=2024-01-01
/// r2, calendar>2024-01-01 none, calendar!=2024-02-28 r2, opening>07:00:00Z r1, opening!=08:00:00Z r2,
/// comma==2024-01-01T00:00:00.5Z r1, list==2024-01-01T00:00:00Z none, list!=2024-01-01T00:00:00Z r3,
/// held[m]>2023-12-31T00:00:00Z none and held[m]!=2024-01-01T00:00:00Z r3; a value object declaring a time value is
/// that value when its `@value` spells one in the clause 4.6.3 form of its datatype, a decimal comma included, and
/// otherwise (an offset, an unpadded date, a day the calendar does not have), as an element of an array value or as a
/// member of an object value, the raw JSON it is, which no time operand is included in and which is unequal to every
/// one
/// Clause: 4.6.3, 4.9, 5.2.9, 5.10.2.4, 5.11.2.4, C.6
#[gtest]
#[tokio::test]
async fn n_csr_qry_9095_a_declared_time_value_is_read_in_its_form_alike_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let ids = [
        namespace.registration_id("r1")?,
        namespace.registration_id("r2")?,
        namespace.registration_id("r3")?,
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
    let since: RawJson = r#"{"@type": "DateTime", "@value": "2024-01-01T00:00:00Z"}"#.parse()?;
    let day: RawJson = r#"{"@type": "Date", "@value": "2024-01-01"}"#.parse()?;
    let opening: RawJson = r#"{"type": "Property", "value": {"@type": "Time", "@value": "08:00:00Z"}}"#.parse()?;
    let comma: RawJson = r#"{"@type": "DateTime", "@value": "2024-01-01T00:00:00,5Z"}"#.parse()?;
    let offset_since: RawJson = r#"{"@type": "DateTime", "@value": "2024-01-01T00:00:00+00:00"}"#.parse()?;
    let short_day: RawJson = r#"{"@type": "Date", "@value": "2024-1-1"}"#.parse()?;
    let offset_opening: RawJson = r#"{"@type": "Time", "@value": "08:00:00+01:00"}"#.parse()?;
    let calendar: RawJson = r#"{"@type": "Date", "@value": "2024-02-30"}"#.parse()?;
    let list: RawJson = r#"[{"@type": "DateTime", "@value": "2024-01-01T00:00:00Z"}]"#.parse()?;
    let held: RawJson = r#"{"m": {"@type": "DateTime", "@value": "2024-01-01T00:00:00Z"}}"#.parse()?;
    let [r1, r2, r3] = &ids;
    let [first, second, third] = registrations.as_slice() else {
        return fail!("three registrations");
    };
    let corpus = [
        CsfRegistration {
            registration: first,
            properties: &[("since", &since), ("day", &day), ("opening", &opening), ("comma", &comma)],
        },
        CsfRegistration {
            registration: second,
            properties: &[
                ("since", &offset_since),
                ("day", &short_day),
                ("opening", &offset_opening),
                ("calendar", &calendar),
            ],
        },
        CsfRegistration {
            registration: third,
            properties: &[("list", &list), ("held", &held)],
        },
    ];
    let cases = [
        CsfCase {
            csf: "since>2023-12-31T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "since!=2024-01-01T00:00:00Z",
            selects: &[r2],
        },
        CsfCase {
            csf: "since==\"2024-01-01T00:00:00+00:00\"",
            selects: &[],
        },
        CsfCase {
            csf: "since==2024-01-01T00:00:00Z,\"x\"",
            selects: &[r1],
        },
        CsfCase {
            csf: "day>2023-12-31",
            selects: &[r1],
        },
        CsfCase {
            csf: "day!=2024-01-01",
            selects: &[r2],
        },
        CsfCase {
            csf: "calendar>2024-01-01",
            selects: &[],
        },
        CsfCase {
            csf: "calendar!=2024-02-28",
            selects: &[r2],
        },
        CsfCase {
            csf: "opening>07:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "opening!=08:00:00Z",
            selects: &[r2],
        },
        CsfCase {
            csf: "comma==2024-01-01T00:00:00.5Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "list==2024-01-01T00:00:00Z",
            selects: &[],
        },
        CsfCase {
            csf: "list!=2024-01-01T00:00:00Z",
            selects: &[r3],
        },
        CsfCase {
            csf: "held[m]>2023-12-31T00:00:00Z",
            selects: &[],
        },
        CsfCase {
            csf: "held[m]!=2024-01-01T00:00:00Z",
            selects: &[r3],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
