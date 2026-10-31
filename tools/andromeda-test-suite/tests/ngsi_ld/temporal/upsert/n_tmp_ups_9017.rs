//! A simplified temporal evolution is recorded and reads back as written.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DateTime,
        GeoValue,
        LanguageMap,
        NgsiLdMediaType,
        OneOrMany,
        RawJson,
        Value,
        VocabTerm,
        evolution::{SimplifiedTemporalInput, TemporalValue, TemporalValues},
        geometry::{Geometry, Position, Shape},
    },
    query::temporal_query,
};

/// Pins: Create Temporal Evolution of an Entity in the simplified temporal representation with one attribute of each of
/// the eight families observed twice answers 201, reads back normalized with each pair as an instance, and with
/// temporalValues as written
/// Clause: 5.2.20, 4.5.9, 5.6.11.3
#[gtest]
#[tokio::test]
async fn n_tmp_ups_9017_a_simplified_temporal_evolution_is_recorded_and_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let simplified_probe_create_id = namespace.urn("SimplifiedProbe", "create")?;
    let simplified_probe_type = namespace.entity_type("SimplifiedProbe")?;
    let first = "2022-08-09T18:25:02Z".parse::<DateTime>()?;
    let second = "2022-08-10T18:25:02Z".parse::<DateTime>()?;
    let point =
        |longitude: f64, latitude: f64| -> Result<GeoValue> { Ok(GeoValue::Geometry(Geometry::from(Shape::Point(Position::try_new(longitude, latitude)?)))) };
    let person = |name: &str| namespace.urn("Person", name);
    let simplified_probe_create = SimplifiedTemporalInput::builder()
        .id(simplified_probe_create_id.clone())
        .entity_type(simplified_probe_type)
        .values(
            "name",
            TemporalValues::Property(vec![
                TemporalValue { value: Value::from("Joe Bloggs"), time: first },
                TemporalValue { value: Value::from("Bill Smith"), time: second },
            ]),
        )
        .values(
            "location",
            TemporalValues::GeoProperty(vec![
                TemporalValue { value: point(13.5, 52.5)?, time: first },
                TemporalValue { value: point(13.25, 52.75)?, time: second },
            ]),
        )
        .values(
            "says",
            TemporalValues::LanguageProperty(vec![
                TemporalValue { value: LanguageMap::try_from_pairs([("en", "yes"), ("fr", "oui")])?, time: first },
                TemporalValue { value: LanguageMap::try_from_pairs([("en", "no"), ("fr", "non")])?, time: second },
            ]),
        )
        .values(
            "period",
            TemporalValues::ListProperty(vec![
                TemporalValue { value: vec![Value::from("First"), Value::from("Second")], time: first },
                TemporalValue { value: vec![Value::from("1st"), Value::from("2nd")], time: second },
            ]),
        )
        .values(
            "parkingTickets",
            TemporalValues::JsonProperty(vec![
                TemporalValue {
                    value: format!(
                        r#"[{{"id": "{namespace}-85a6cc52-0589-45f9", "value": "Overstay 60 minutes"}}, {{"id": "{namespace}-x5c56s-0589-45f9", "value": "Overstay 45 minutes"}}]"#
                    )
                    .parse::<RawJson>()?,
                    time: first,
                },
                TemporalValue {
                    value: format!(r#"[{{"id": "{namespace}-y7d21t-0589-45f9", "value": "Overstay 30 minutes"}}]"#).parse::<RawJson>()?,
                    time: second,
                },
            ]),
        )
        .values(
            "gender",
            TemporalValues::VocabProperty(vec![
                TemporalValue { value: OneOrMany::one("Male".parse::<VocabTerm>()?), time: first },
                TemporalValue { value: OneOrMany::one("Female".parse::<VocabTerm>()?), time: second },
            ]),
        )
        .values(
            "spouse",
            TemporalValues::Relationship(vec![
                TemporalValue { value: OneOrMany::one(person("123455")?), time: first },
                TemporalValue { value: OneOrMany::one(person("999999")?), time: second },
            ]),
        )
        .values(
            "membersPresent",
            TemporalValues::ListRelationship(vec![
                TemporalValue { value: vec![person("alice")?, person("bob")?], time: first },
                TemporalValue { value: vec![person("alice")?, person("eve")?], time: second },
            ]),
        )
        .build()?;
    broker
        .temporal()
        .create_or_update_simplified(&simplified_probe_create)
        .send()
        .await?
        .created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&simplified_probe_create_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("name", all!(len(eq(2)), nth(1, any_attribute().value("Bill Smith").observed_at(second))))
    );
    expect_that!(
        evolution,
        has_evolution("says", nth(0, any_attribute().language("en", "yes").language("fr", "oui")))
    );
    let retrieved_2 = client
        .temporal()
        .retrieve_values(&simplified_probe_create_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(&retrieved_2.body().attributes, eq(&simplified_probe_create.as_entity().attributes));
    Ok(())
}
