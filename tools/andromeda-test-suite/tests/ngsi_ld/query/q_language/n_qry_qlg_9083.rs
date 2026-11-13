//! A q corpus selects alike in a query and a subscription.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
    value::object,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property, Relationship, Value};

/// Pins: for three Vehicles, Query Entities and a Subscription select the same Vehicles, the ones clause 4.9 gives,
/// for every term of a corpus spanning existence, every comparison operator, a range, value lists, patterns, a
/// Relationship object quoted and unquoted, a Boolean, a compound member, a sub-attribute, the `unitCode`,
/// `observedAt` and `expiresAt` members, mismatched operand types, and `;`, `|` and a group
/// Clause: 4.9, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9083_a_q_corpus_selects_alike_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fast_id = namespace.urn("Vehicle", "fast")?;
    let slow_id = namespace.urn("Vehicle", "slow")?;
    let bare_id = namespace.urn("Vehicle", "bare")?;
    let parking_id = namespace.urn("Parking", 1)?;
    let entities = [
        Entity::builder()
            .id(fast_id.clone())
            .entity_type(vehicle_type.clone())
            .property(
                "speed",
                Property::builder(80)
                    .unit_code(UnitCode::Kmh)
                    .observed_at("2024-06-01T00:00:00Z".parse()?)
                    .expires_at("2199-01-01T00:00:00Z".parse()?)
                    .property("accuracy", Value::Number("0.5".parse()?)),
            )
            .property("brandName", "Mercedes")
            .relationship("isParked", Relationship::builder(parking_id.clone()))
            .property("active", true)
            .property("address", object([("city", Value::from("Berlin"))]))
            .build()?,
        Entity::builder()
            .id(slow_id.clone())
            .entity_type(vehicle_type.clone())
            .property(
                "speed",
                Property::builder(50)
                    .unit_code(UnitCode::Mts)
                    .observed_at("2023-06-01T00:00:00Z".parse()?)
                    .property("accuracy", Value::Number("0.9".parse()?)),
            )
            .property("brandName", "BMW")
            .property("active", false)
            .property("address", object([("city", Value::from("Paris"))]))
            .build()?,
        Entity::builder()
            .id(bare_id.clone())
            .entity_type(vehicle_type.clone())
            .property("brandName", "Mercedes")
            .build()?,
    ];
    let parked_uri = format!("isParked=={parking_id}");
    let parked_text = format!("isParked==\"{parking_id}\"");
    let both: &[_] = &[&fast_id, &slow_id];
    let cases = [
        QCase { q: "speed", selects: both },
        QCase {
            q: "speed>60",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed<=50",
            selects: &[&slow_id],
        },
        QCase {
            q: "speed==50..90",
            selects: both,
        },
        QCase {
            q: "speed!=50..60",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed!=50",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed==50,80",
            selects: both,
        },
        QCase {
            q: "speed==\"80\"",
            selects: &[],
        },
        QCase {
            q: "speed!=\"80\"",
            selects: both,
        },
        QCase {
            q: "brandName==\"Mercedes\"",
            selects: &[&fast_id, &bare_id],
        },
        QCase {
            q: "brandName==\"BMW\",\"Audi\"",
            selects: &[&slow_id],
        },
        QCase {
            q: "brandName>\"C\"",
            selects: &[&fast_id, &bare_id],
        },
        QCase {
            q: "brandName~=\"^Merc\"",
            selects: &[&fast_id, &bare_id],
        },
        QCase {
            q: "brandName!~=\"^Merc\"",
            selects: &[&slow_id],
        },
        QCase {
            q: &parked_uri,
            selects: &[&fast_id],
        },
        QCase {
            q: &parked_text,
            selects: &[&fast_id],
        },
        QCase {
            q: "isParked~=\"Parking\"",
            selects: &[],
        },
        QCase {
            q: "active==true",
            selects: &[&fast_id],
        },
        QCase {
            q: "active!=true",
            selects: &[&slow_id],
        },
        QCase {
            q: "address[city]==\"Berlin\"",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed.accuracy<0.6",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed.unitCode==\"KMH\"",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed.observedAt>2024-01-01T00:00:00Z",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed.expiresAt>2150-01-01T00:00:00Z",
            selects: &[&fast_id],
        },
        QCase {
            q: "speed>60|brandName==\"BMW\"",
            selects: both,
        },
        QCase {
            q: "speed>60;brandName==\"BMW\"",
            selects: &[],
        },
        QCase {
            q: "(speed<60|active==true);brandName==\"Mercedes\"",
            selects: &[&fast_id],
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &vehicle_type, &entities, &cases).await
}
