//! A page of several types holds the Entities of every type once, in identifier order.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, instant_ahead::minutes_ahead, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryMethod, ResultsCount},
    },
    model::{
        DateTime,
        Entity,
        EntityId,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{
        EntitySelector,
        IdPattern,
        Locality,
        builders::SelectorTypeBuilder,
        geoquery::{self, GeoQuery},
        q::{self, Query},
        scope_query::ScopeQuery,
        type_selection,
    },
};

/// One selection of the Sensors and the Gateways, and the Entities it selects.
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
    /// The identifiers it selects, in identifier order.
    expected: Vec<EntityId>,
}

/// The page size every walk reads.
const PAGE: usize = 3;

/// Pins: among 24 Devices whose identifiers interleave a Sensor, an Entity of both types and a Gateway in turn, beside
/// two Devices of a third type and a Gateway whose `expiresAt` is a minute past, Query Entities by `Sensor,Gateway`
/// and by `Gateway,Sensor`, alone and beside a `q`, an unanchored `idPattern`, a `scopeQ` and a `near` geoquery,
/// answers every offset page and every page a next or prev link reaches with the selected Entities in identifier
/// order, each once, `NGSILD-Results-Count` equal to their number
/// Clause: 4.17, 4.22, 5.5.9, 5.7.2, 6.3.10, 6.3.13
#[gtest]
#[tokio::test]
async fn a_qry_pag_9102_a_page_of_several_types_holds_the_entities_of_every_type_once_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let gateway_type = namespace.entity_type("Gateway")?;
    let other_type = namespace.entity_type("Other")?;
    let device = |n: u32| namespace.urn("Device", n);
    let place = |n: u32| Position::try_new(8.6 + f64::from(n) * 0.01, 49.39);
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let mut entities = Vec::new();
    for n in 1..=27 {
        let types = match (n, n % 3) {
            (25 | 26, _) => vec![other_type.clone()],
            (27, _) | (_, 0) => vec![gateway_type.clone()],
            (_, 1) => vec![sensor_type.clone()],
            (_, _) => vec![sensor_type.clone(), gateway_type.clone()],
        };
        let builder = Entity::builder()
            .id(device(n)?)
            .entity_type(types)
            .property("status", if n % 4 == 0 { "fail" } else { "ok" })
            .scope(if n % 2 == 1 { "/North" } else { "/South" })
            .geo_property("location", Shape::Point(place(n)?));
        entities.push(if n == 27 {
            builder.expires_at(a_minute_ago).build()?
        } else {
            builder.build()?
        });
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let devices = |numbers: &mut dyn Iterator<Item = u32>| -> Result<Vec<EntityId>> {
        let mut ids = numbers.map(|n| -> Result<EntityId> { Ok(device(n)?) }).collect::<Result<Vec<EntityId>>>()?;
        ids.sort_by_key(ToString::to_string);
        Ok(ids)
    };
    let both = || -> Result<Vec<EntitySelector>> {
        Ok(vec![EntitySelector::new(
            SelectorTypeBuilder::from(type_selection::entity_type(&sensor_type).or(&gateway_type)).build()?,
        )])
    };
    let unanchored: IdPattern = format!(".*:Device:{}-1[0-9]$", namespace.token()).parse()?;
    let cases = [
        Case {
            name: "types",
            selectors: both()?,
            q: None,
            geo_q: None,
            scope_q: None,
            expected: devices(&mut (1..=24))?,
        },
        Case {
            name: "types reversed",
            selectors: vec![EntitySelector::new(
                SelectorTypeBuilder::from(type_selection::entity_type(&gateway_type).or(&sensor_type)).build()?,
            )],
            q: None,
            geo_q: None,
            scope_q: None,
            expected: devices(&mut (1..=24))?,
        },
        Case {
            name: "q",
            selectors: both()?,
            q: Some(q::attr("status").eq("fail").build()?),
            geo_q: None,
            scope_q: None,
            expected: devices(&mut (1..=24).filter(|n| n % 4 == 0))?,
        },
        Case {
            name: "unanchored id-pattern",
            selectors: vec![
                EntitySelector::new(SelectorTypeBuilder::from(type_selection::entity_type(&sensor_type).or(&gateway_type)).build()?)
                    .with_id_pattern(unanchored),
            ],
            q: None,
            geo_q: None,
            scope_q: None,
            expected: devices(&mut (10..=19))?,
        },
        Case {
            name: "scope",
            selectors: both()?,
            q: None,
            geo_q: None,
            scope_q: Some("/North".parse::<ScopeQuery>()?),
            expected: devices(&mut (1..=24).filter(|n| n % 2 == 1))?,
        },
        // The Devices stand 0.01 degrees of longitude apart, about 725 m at this latitude, so 1 000 m from the sixth
        // reaches the fifth and the seventh alone.
        Case {
            name: "near",
            selectors: both()?,
            q: None,
            geo_q: Some(geoquery::near_max(1000, Shape::Point(place(6)?)).build()?),
            scope_q: None,
            expected: devices(&mut (5..=7))?,
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
                .count(ResultsCount::Include)
        };
        let total = Some(u64::try_from(case.expected.len())?);
        for offset in (0..=case.expected.len()).step_by(PAGE).chain([40]) {
            let page = query().limit(u32::try_from(PAGE)?).offset(u32::try_from(offset)?).send().await?;
            let held = case.expected.get(offset..case.expected.len().min(offset + PAGE)).unwrap_or_default();
            expect_that!(page.items(), ids_in_order(held), "{} at offset {offset}", case.name);
            expect_that!(page.total(), eq(total), "{} total at offset {offset}", case.name);
        }
        let mut page = query().limit(u32::try_from(PAGE)?).locality(Locality::Local).send().await?;
        expect_that!(page.total(), eq(total), "{} total of the first cursor page", case.name);
        let mut forward = vec![page.items().iter().map(|entity| entity.id.clone()).collect::<Vec<EntityId>>()];
        while let Some(request) = page.next_page() {
            let next = json.send(&request).await?;
            expect_that!(next.total(), eq(total), "{} total of a next page", case.name);
            page = next;
            forward.push(page.items().iter().map(|entity| entity.id.clone()).collect());
        }
        expect_that!(forward.concat(), eq(&case.expected), "{} walked by next links", case.name);
        let mut backward = vec![page.items().iter().map(|entity| entity.id.clone()).collect::<Vec<EntityId>>()];
        while let Some(request) = page.prev_page() {
            page = json.send(&request).await?;
            backward.push(page.items().iter().map(|entity| entity.id.clone()).collect());
        }
        backward.reverse();
        expect_that!(backward, eq(&forward), "{} walked back by prev links", case.name);
    }
    Ok(())
}
