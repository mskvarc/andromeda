//! The query cases of an inclusive context source registered for one query operation alone: a query of the Vehicle
//! `urn:ngsi-ld:Vehicle:<ns>-1` by id and type, sent by `GET /entities` to a source registered for `queryEntity` or by
//! `POST /entityOperations/query` to one registered for `queryBatch`, is forwarded in that binding and answers the
//! source's Vehicle.

use crate::distributed_operations::{
    fork_rule::{ForkRule, QueriedEntity},
    forwarding_source::{listed_ids, register, registration_id, serve, source_endpoint, source_path, stub, stub_query_of, unregister, vehicle_reply},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 4] = ["Vehicle", "brandName", "isParked", "providedBy"];

/// The query binding a case sends and the one operation its registration names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Binding {
    /// `GET /entities`, the registration naming `queryEntity`.
    QueryEntity,
    /// `POST /entityOperations/query`, the registration naming `queryBatch`.
    QueryBatch,
}

/// The case of `binding` in a private tenant: registers the vocabulary and every Vehicle at the source for the
/// binding's operation alone, starts the source and stubs the Vehicle it answers in that binding, queries the Vehicle
/// by id and type, requires 200 and the one Vehicle, and deletes the registration (204).
pub(crate) async fn expect_forwarded_query(binding: Binding) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let source = ContextSource::refusing()?;
    let operation = match binding {
        Binding::QueryEntity => DistributedOperation::QueryEntity,
        Binding::QueryBatch => DistributedOperation::QueryBatch,
    };
    let registration = CSourceRegistration::builder(source_endpoint(&source, &namespace, "")?.as_str())
        .id(registration_id(&namespace, 1)?)
        .information([RegistrationInfo::builder().entities(["Vehicle"]).build()?])
        .operations([operation])
        .build()?;
    let _registration = register(&broker, &vocabulary, &registration).await?;
    let source = serve(source).await?;
    let id = namespace.urn_text("Vehicle", 1);
    let reply = json!([vehicle_reply(&namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")]);
    let reader = broker.with_context(vocabulary.context())?;
    let received = match binding {
        Binding::QueryEntity => {
            let rule = ForkRule::query(
                Method::GET,
                &source_path(&namespace, "/ngsi-ld/v1/entities"),
                &["id", &id, "type", "vehicle"],
                Some(&["brandname", "id", "isparked", "type"]),
            );
            stub(&source, rule, StatusCode::OK, Some(&reply)).await?;
            reader
                .with_accept(Accept::any())
                .with_responses()
                .entities()
                .query()
                .ids(vec![namespace.urn("Vehicle", 1)?])
                .entity_type("Vehicle")
                .send()
                .await
        }
        Binding::QueryBatch => {
            let rule = ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/query"));
            let queried = QueriedEntity {
                entity_type: "Vehicle".to_owned(),
                id: namespace.urn_text("Vehicle", 1),
            };
            stub_query_of(&source, rule, queried, StatusCode::OK, Some(&reply)).await?;
            reader
                .with_accept(NgsiLdMediaType::Json)
                .with_responses()
                .entities()
                .query()
                .entity_type("Vehicle")
                .ids(vec![namespace.urn("Vehicle", 1)?])
                .method(QueryMethod::Post)
                .send()
                .await
        }
    };
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(listed_ids(&body), contains(eq(&id.as_str())));
    unregister(&broker, &namespace, 1).await
}
