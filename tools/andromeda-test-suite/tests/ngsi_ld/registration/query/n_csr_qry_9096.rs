//! An instant and a date of the ISO 8601 year `0000` in a Context Source Property read alike in a discovery query and a
//! subscription to registrations, as the time values they spell.

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

/// Pins: over registration r1 (`seen` a Property node whose `observedAt` is 0000-01-01T00:00:00Z, `epoch` declared the
/// `DateTime` 0000-02-29T12:00:00Z, `dawn` declared the `Date` 0000-02-29) and r2 (`epoch` declared the `DateTime`
/// 0001-01-01T00:00:00Z, `dawn` declared the `Date` 0000-02-30), Query Context Source Registrations and a Context Source
/// Registration Subscription with each csf select alike: seen.observedAt<0001-01-01T00:00:00Z r1,
/// epoch<0001-01-01T00:00:00Z r1, epoch==0000-02-29T12:00:00Z r1, epoch>0000-12-31T23:59:59Z r2, dawn<0001-01-01 r1,
/// dawn==0000-02-29 r1 and dawn!=0000-02-29 r2; ISO 8601, which clause 4.6.3 defines its `DateTime` by, counts the year
/// before year 1 as `0000`, a leap year, so an instant or a date of that year is the time value it spells and precedes
/// every one of year 1, and `0000-02-30` names no day
/// Clause: 4.6.3, 4.8, 4.9, 5.2.9, 5.10.2.4, 5.11.2.4, C.6
#[gtest]
#[tokio::test]
async fn n_csr_qry_9096_a_year_zero_time_value_is_read_alike_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let ids = [namespace.registration_id("r1")?, namespace.registration_id("r2")?];
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
    let seen: RawJson = r#"{"type": "Property", "value": 1, "observedAt": "0000-01-01T00:00:00Z"}"#.parse()?;
    let epoch: RawJson = r#"{"@type": "DateTime", "@value": "0000-02-29T12:00:00Z"}"#.parse()?;
    let dawn: RawJson = r#"{"@type": "Date", "@value": "0000-02-29"}"#.parse()?;
    let year_one: RawJson = r#"{"@type": "DateTime", "@value": "0001-01-01T00:00:00Z"}"#.parse()?;
    let no_day: RawJson = r#"{"@type": "Date", "@value": "0000-02-30"}"#.parse()?;
    let [r1, r2] = &ids;
    let [first, second] = registrations.as_slice() else {
        return fail!("two registrations");
    };
    let corpus = [
        CsfRegistration {
            registration: first,
            properties: &[("seen", &seen), ("epoch", &epoch), ("dawn", &dawn)],
        },
        CsfRegistration {
            registration: second,
            properties: &[("epoch", &year_one), ("dawn", &no_day)],
        },
    ];
    let cases = [
        CsfCase {
            csf: "seen.observedAt<0001-01-01T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "epoch<0001-01-01T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "epoch==0000-02-29T12:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "epoch>0000-12-31T23:59:59Z",
            selects: &[r2],
        },
        CsfCase {
            csf: "dawn<0001-01-01",
            selects: &[r1],
        },
        CsfCase {
            csf: "dawn==0000-02-29",
            selects: &[r1],
        },
        CsfCase {
            csf: "dawn!=0000-02-29",
            selects: &[r2],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
