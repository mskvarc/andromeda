//! A forwarded batch query carries the client filters.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryEntitiesParameters, QueryEntitiesPostParameters, SystemAttributes},
    },
    model::{
        AttributeName,
        Entity,
        GeoProperty,
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery, language_filter::LanguageFilter, q, q::Query, scope_query::ScopeQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::QueryDocument,
};
use std::str;

/// Pins: Query Entities by GET with `type`, `q`, a geoquery, `scopeQ`, `csf`, a ranked `lang`, `attrs`,
/// `options=sysAttrs` and `splitEntities=false` over an inclusive source advertising only `queryBatch` and one
/// advertising only `queryEntity` reaches the first as a POST whose Query body holds `q`, `geoQ`, `scopeQ`, `csf`,
/// `lang` and `attrs` equal to the parameters the second got, with `options=sysAttrs` in its URI, and answers the
/// Entities of both
/// Clause: 5.7.2.4, 6.23.1, 6.23.3.1, Table 5.2.23-1, 6.3.11
/// Recorded: both bindings carry the same filters, since clause 5.7.2.4 forwards them whichever operation the
/// registration advertises
#[gtest]
#[tokio::test]
async fn n_fed_qry_9030_a_forwarded_batch_query_carries_the_client_filters() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let batch_vehicle_id = namespace.urn("Vehicle", "batch")?;
    let get_vehicle_id = namespace.urn("Vehicle", "get")?;
    let information = RegistrationInfo::builder()
        .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
        .build()?;
    let batch_registration_id = namespace.registration_id("batch")?;
    let batch_registration = CSourceRegistration::builder(source.endpoint("b")?)
        .id(batch_registration_id.clone())
        .information([information.clone()])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryBatch])
        .property("sourceGroup", "fleet")
        .build()?;
    let get_registration_id = namespace.registration_id("get")?;
    let get_registration = CSourceRegistration::builder(source.endpoint("g")?)
        .id(get_registration_id.clone())
        .information([information])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .property("sourceGroup", "fleet")
        .build()?;
    broker.registrations().register(&batch_registration).send().await?;
    broker.registrations().register(&get_registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registrations_cleanup = Cleanup::of(&broker).registration(&batch_registration_id).registration(&get_registration_id);
    let answered_entity = |id| {
        Entity::output_builder()
            .id(id)
            .entity_type(vehicle_type.clone())
            .scope("/Area/A")
            .property("speed", 56)
            .geo_property("location", GeoProperty::builder(Shape::Point(Position::try_new(10.08, 50.03)?)))
            .build()
    };
    source
        .answer_when(
            SourceRequest::new(Method::POST, "/b/ngsi-ld/v1/entityOperations/query").body(BodyPattern::Contains(vehicle_type.to_string())),
            source_answer::entities(&[answered_entity(batch_vehicle_id.clone())?]),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/g/ngsi-ld/v1/entities").query_containing("type", &vehicle_type),
            source_answer::entities(&[answered_entity(get_vehicle_id.clone())?]),
        )
        .await?;
    let speed_query: Query = q::attr("speed").gt(10).build()?;
    let source_filter: Query = q::attr("sourceGroup").eq("fleet").build()?;
    let geo_query = geoquery::intersects(Shape::Point(Position::try_new(10.08, 50.03)?))
        .geoproperty("location")
        .build()?;
    let scope_query: ScopeQuery = "/Area/A".parse()?;
    let lang: LanguageFilter = "fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5".parse()?;
    let attrs = vec!["speed".parse::<AttributeName>()?, "location".parse()?];
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(speed_query.clone())
        .geo_q(geo_query.clone())
        .scope_q(scope_query.clone())
        .csf(source_filter.clone())
        .lang(lang.clone())
        .attrs(attrs.clone())
        .system_attributes(SystemAttributes::Include)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&batch_vehicle_id)));
    expect_that!(page.items(), contains(has_id(&get_vehicle_id)));
    let posted = source
        .received_matching(&SourceRequest::new(Method::POST, "/b/ngsi-ld/v1/entityOperations/query").body(BodyPattern::Contains(vehicle_type.to_string())))
        .await?;
    let posted = posted.first().or_fail()?;
    let batch_query: QueryDocument = QueryDocument::parse(str::from_utf8(&posted.body)?)?;
    let batch_uri = QueryEntitiesPostParameters::parse(posted.url.query().unwrap_or_default())?;
    let got = source
        .received_matching(&SourceRequest::new(Method::GET, "/g/ngsi-ld/v1/entities").query_containing("type", &vehicle_type))
        .await?;
    let got = got.first().or_fail()?;
    let get_uri = QueryEntitiesParameters::parse(got.url.query().unwrap_or_default())?;
    expect_that!(batch_query.q(), some(eq(&speed_query)));
    expect_that!(get_uri.selection.q.as_ref(), eq(batch_query.q()));
    expect_that!(batch_query.geo_q(), some(eq(&geo_query)));
    expect_that!(get_uri.selection.geo_q.as_ref(), eq(batch_query.geo_q()));
    expect_that!(batch_query.scope_q(), some(eq(&scope_query)));
    expect_that!(get_uri.selection.scope_q.as_ref(), eq(batch_query.scope_q()));
    expect_that!(batch_query.csf(), some(eq(&source_filter)));
    expect_that!(get_uri.selection.csf.as_ref(), eq(batch_query.csf()));
    expect_that!(batch_query.lang(), some(eq(&lang)));
    expect_that!(get_uri.selection.lang.as_ref(), eq(batch_query.lang()));
    expect_that!(batch_query.attrs(), eq(attrs.as_slice()));
    expect_that!(get_uri.selection.attrs, eq(batch_query.attrs()));
    expect_that!(batch_uri.system_attributes, eq(SystemAttributes::Include));
    expect_that!(get_uri.system_attributes, eq(batch_uri.system_attributes));
    Ok(())
}
