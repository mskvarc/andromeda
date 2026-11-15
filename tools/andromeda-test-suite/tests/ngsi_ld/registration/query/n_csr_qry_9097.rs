//! A time interval member is the `DateTime` it holds alike in a discovery query and a subscription to registrations.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    csf_parity::{CsfCase, CsfRegistration, expect_registration_query_and_subscription_alike},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval};

/// Pins: over registrations r1 (`observationInterval` from 2024-01-01T00:00:00Z to 2025-01-01T00:00:00Z), r2
/// (`observationInterval` open from 2024-03-01T00:00:00Z) and r3 (`managementInterval` from 2024-01-01T00:00:00Z to
/// 2024-02-01T00:00:00Z), Query Context Source Registrations and a Context Source Registration Subscription with each
/// csf select alike: observationInterval[startAt]<2024-02-01T00:00:00Z r1,
/// observationInterval[startAt]>=2024-02-01T00:00:00Z r2, observationInterval[startAt]==2024-01-01T00:00:00Z r1,
/// observationInterval[startAt]==2024-01-01T00:00:00.000Z r1, observationInterval[startAt]!=2024-01-01T00:00:00Z r2,
/// observationInterval[startAt]=="2024-01-01T00:00:00Z" none, observationInterval[endAt] r1,
/// observationInterval[endAt]>2024-06-01T00:00:00Z r1, managementInterval[endAt]<2024-06-01T00:00:00Z r3 and
/// managementInterval[startAt]==2024-01-01T00:00:00Z r3; the `startAt` and `endAt` of a time interval are the
/// `DateTime` Table 5.2.11-1 types them as, which a `DateTime` operand compares with in temporal order and a String
/// operand never equals, and an open interval has no `endAt`
/// Clause: 4.6.3, 4.9, 5.2.9, 5.2.11, 5.10.2.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9097_a_time_interval_member_is_the_date_time_it_holds_alike_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = namespace.entity_type("Device")?;
    let ids = [
        namespace.registration_id("r1")?,
        namespace.registration_id("r2")?,
        namespace.registration_id("r3")?,
    ];
    let [r1, r2, r3] = &ids;
    let registration = |id| -> Result<_> {
        Ok(CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
                .build()?]))
    };
    let closed = registration(r1.clone())?
        .observation_interval(TimeInterval::between("2024-01-01T00:00:00Z".parse()?, "2025-01-01T00:00:00Z".parse()?))
        .build()?;
    let open = registration(r2.clone())?
        .observation_interval(TimeInterval::starting_at("2024-03-01T00:00:00Z".parse()?))
        .build()?;
    let managed = registration(r3.clone())?
        .management_interval(TimeInterval::between("2024-01-01T00:00:00Z".parse()?, "2024-02-01T00:00:00Z".parse()?))
        .build()?;
    let corpus = [
        CsfRegistration {
            registration: &closed,
            properties: &[],
        },
        CsfRegistration {
            registration: &open,
            properties: &[],
        },
        CsfRegistration {
            registration: &managed,
            properties: &[],
        },
    ];
    let cases = [
        CsfCase {
            csf: "observationInterval[startAt]<2024-02-01T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "observationInterval[startAt]>=2024-02-01T00:00:00Z",
            selects: &[r2],
        },
        CsfCase {
            csf: "observationInterval[startAt]==2024-01-01T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "observationInterval[startAt]==2024-01-01T00:00:00.000Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "observationInterval[startAt]!=2024-01-01T00:00:00Z",
            selects: &[r2],
        },
        CsfCase {
            csf: "observationInterval[startAt]==\"2024-01-01T00:00:00Z\"",
            selects: &[],
        },
        CsfCase {
            csf: "observationInterval[endAt]",
            selects: &[r1],
        },
        CsfCase {
            csf: "observationInterval[endAt]>2024-06-01T00:00:00Z",
            selects: &[r1],
        },
        CsfCase {
            csf: "managementInterval[endAt]<2024-06-01T00:00:00Z",
            selects: &[r3],
        },
        CsfCase {
            csf: "managementInterval[startAt]==2024-01-01T00:00:00Z",
            selects: &[r3],
        },
    ];
    expect_registration_query_and_subscription_alike(&broker, &namespace, &device_type, &corpus, &cases).await
}
