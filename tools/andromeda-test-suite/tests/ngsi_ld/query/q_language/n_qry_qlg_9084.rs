//! A pattern skips a declared Date in a query and a subscription alike.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Property};

/// Pins: for a Diary whose `d` is "2024-01-15" declared an NGSI-LD Date and a Diary whose `d` is the same text with
/// no `valueType`, Query Entities and a Subscription with q d~="2024.*" select only the undeclared one, and with
/// q d!~="1999.*" only the undeclared one too; both pattern operators answer not matching for a target value whose
/// data type is not String, and the declared value is a Date
/// Clause: 4.9, 4.6.3, 5.8.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9084_a_pattern_skips_a_declared_date_in_a_query_and_a_subscription_alike() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let diary_type = namespace.entity_type("Diary")?;
    let dated_id = namespace.urn("Diary", "dated")?;
    let text_id = namespace.urn("Diary", "text")?;
    let entities = [
        Entity::builder()
            .id(dated_id.clone())
            .entity_type(diary_type.clone())
            .property("d", Property::builder("2024-01-15").value_type("https://uri.etsi.org/ngsi-ld/Date".parse()?))
            .build()?,
        Entity::builder()
            .id(text_id.clone())
            .entity_type(diary_type.clone())
            .property("d", "2024-01-15")
            .build()?,
    ];
    let cases = [
        QCase {
            q: "d~=\"2024.*\"",
            selects: &[&text_id],
        },
        QCase {
            q: "d!~=\"1999.*\"",
            selects: &[&text_id],
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &diary_type, &entities, &cases).await
}
