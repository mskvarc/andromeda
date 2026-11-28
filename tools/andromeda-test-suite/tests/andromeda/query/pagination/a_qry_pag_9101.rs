//! Every count of a selection is the number of Entities its pages walk.

use andromeda_test_suite::{broker, instant_ahead::minutes_ahead, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{
        DateTime,
        Entity,
        NgsiLdMediaType,
        OneOrMany,
        geometry::{Position, Shape},
    },
    query::{
        EntitySelector,
        Locality,
        builders::SelectorTypeBuilder,
        geoquery::{self, GeoQuery},
        q::{self, Query},
        scope_query::ScopeQuery,
        type_selection,
    },
};

/// One selection and the Entities it selects.
struct Case {
    /// The selection's name, for the failure message.
    name: &'static str,
    /// The query's selectors.
    selectors: Vec<EntitySelector>,
    /// The `q` of the query.
    q: Option<Query>,
    /// The geoquery of the query.
    geo_q: Option<GeoQuery>,
    /// The `scopeQ` of the query.
    scope_q: Option<ScopeQuery>,
    /// The identifiers it selects, sorted.
    expected: Vec<String>,
}

/// The page size the walk reads.
const PAGE: u32 = 4;

/// Pins: right after one batch creates fourteen Sensors, three Gateways and an Entity of both types, the fourteenth
/// Sensor's `expiresAt` a minute past, and a second batch deletes the first three Sensors, Query Entities with
/// `count=true` answers `NGSILD-Results-Count` equal to the number of Entities its pages walk, with `limit=0`, on the
/// first page of one, on a later page, past the last page and on the page a next link reaches, for the Sensor type
/// alone, both types, a `q`, identifiers, an `idPattern`, a `scopeQ` and a `near` geoquery beside the Sensor type
/// Clause: 4.17, 4.22, 5.5.9, 5.7.2, 6.3.10, 6.3.13
#[gtest]
#[tokio::test]
async fn a_qry_pag_9101_every_count_of_a_selection_is_the_number_of_entities_its_pages_walk() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let gateway_type = namespace.entity_type("Gateway")?;
    let sensor = |n: u32| namespace.urn("Sensor", n);
    let gateway = |n: u32| namespace.urn("Gateway", n);
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let place = |n: u32| Position::try_new(8.6 + f64::from(n) * 0.01, 49.39);
    let mut entities = Vec::new();
    for n in 1..=14 {
        let mut types = vec![sensor_type.clone()];
        if n == 13 {
            types.push(gateway_type.clone());
        }
        let builder = Entity::builder()
            .id(sensor(n)?)
            .entity_type(types)
            .property("rank", i64::from(n))
            .scope(if n % 2 == 1 { "/North" } else { "/South" })
            .geo_property("location", Shape::Point(place(n)?));
        entities.push(if n == 14 {
            builder.expires_at(a_minute_ago).build()?
        } else {
            builder.build()?
        });
    }
    for n in 1..=3 {
        entities.push(
            Entity::builder()
                .id(gateway(n)?)
                .entity_type(gateway_type.clone())
                .property("rank", i64::from(n))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let mut deleted = Vec::new();
    for n in 1..=3 {
        deleted.push(sensor(n)?);
    }
    broker.entities().batch_delete(&deleted).send().await?.done()?;
    let sensors = |range: &[u32]| -> Result<Vec<String>> { range.iter().map(|&n| -> Result<String> { Ok(sensor(n)?.to_string()) }).collect() };
    let sorted = |mut ids: Vec<String>| {
        ids.sort();
        ids
    };
    let of_sensors = || -> Result<Vec<EntitySelector>> { Ok(vec![EntitySelector::new(SelectorTypeBuilder::from(&sensor_type).build()?)]) };
    let standing: Vec<u32> = (4..=13).collect();
    let mut both_types = sensors(&standing)?;
    for n in 1..=3 {
        both_types.push(gateway(n)?.to_string());
    }
    let cases = [
        Case {
            name: "type",
            selectors: of_sensors()?,
            q: None,
            geo_q: None,
            scope_q: None,
            expected: sorted(sensors(&standing)?),
        },
        Case {
            name: "two-types",
            selectors: vec![EntitySelector::new(
                SelectorTypeBuilder::from(type_selection::entity_type(&sensor_type).or(&gateway_type)).build()?,
            )],
            q: None,
            geo_q: None,
            scope_q: None,
            expected: sorted(both_types),
        },
        Case {
            name: "q",
            selectors: of_sensors()?,
            q: Some(q::attr("rank").gt(8).build()?),
            geo_q: None,
            scope_q: None,
            expected: sorted(sensors(&[9, 10, 11, 12, 13])?),
        },
        Case {
            name: "ids",
            selectors: vec![
                EntitySelector::new(SelectorTypeBuilder::from(&sensor_type).build()?).with_ids(OneOrMany::try_from(vec![
                    sensor(2)?,
                    sensor(5)?,
                    sensor(7)?,
                    sensor(14)?,
                ])?),
            ],
            q: None,
            geo_q: None,
            scope_q: None,
            expected: sorted(sensors(&[5, 7])?),
        },
        Case {
            name: "id-pattern",
            selectors: vec![
                EntitySelector::new(SelectorTypeBuilder::from(&sensor_type).build()?).with_id_pattern(namespace.id_pattern_ending("Sensor", "1[0-9]$")?),
            ],
            q: None,
            geo_q: None,
            scope_q: None,
            expected: sorted(sensors(&[10, 11, 12, 13])?),
        },
        Case {
            name: "scope",
            selectors: of_sensors()?,
            q: None,
            geo_q: None,
            scope_q: Some("/North".parse::<ScopeQuery>()?),
            expected: sorted(sensors(&[5, 7, 9, 11, 13])?),
        },
        // The Sensors stand 0.01 degrees of longitude apart, about 725 m at this latitude, so 1 000 m from the sixth
        // reaches the fifth and the seventh alone.
        Case {
            name: "near",
            selectors: of_sensors()?,
            q: None,
            geo_q: Some(geoquery::near_max(1000, Shape::Point(place(6)?)).build()?),
            scope_q: None,
            expected: sorted(sensors(&[5, 6, 7])?),
        },
    ];
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let queried = json.entities();
    for case in &cases {
        let query = || {
            queried
                .query()
                .method(QueryMethod::Post)
                .selectors(case.selectors.clone())
                .maybe_q(case.q.clone())
                .maybe_geo_q(case.geo_q.clone())
                .maybe_scope_q(case.scope_q.clone())
        };
        let mut walked = Vec::new();
        for offset in (0..).step_by(usize::try_from(PAGE)?) {
            let page = query().limit(PAGE).offset(offset).send().await?;
            walked.extend(page.items().iter().map(|entity| entity.id.to_string()));
            if page.items().len() < usize::try_from(PAGE)? {
                break;
            }
        }
        let walked = sorted(walked);
        expect_that!(&walked, eq(&case.expected), "{}", case.name);
        let total = Some(u64::try_from(walked.len())?);
        for (window, limit, offset) in [
            ("limit 0", 0, 0),
            ("first of one", 1, 0),
            ("later page", PAGE, PAGE),
            ("past the last page", PAGE, 40),
        ] {
            let page = query().limit(limit).offset(offset).count(ResultsCount::Include).send().await?;
            expect_that!(page.total(), eq(total), "{} {window}", case.name);
        }
        let first = query().limit(PAGE).count(ResultsCount::Include).locality(Locality::Local).send().await?;
        expect_that!(first.total(), eq(total), "{} first cursor page", case.name);
        match first.next_page() {
            Some(request) => {
                let next = json.send(&request).await?;
                expect_that!(next.total(), eq(total), "{} next link", case.name);
            }
            None => expect_that!(walked.len(), le(usize::try_from(PAGE)?), "{} has a next page", case.name),
        }
    }
    Ok(())
}
