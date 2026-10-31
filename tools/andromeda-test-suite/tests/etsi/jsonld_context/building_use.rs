//! The use of a hosted `@context` that lists remote documents: each document maps the simple Building terms into the
//! case's namespace, and a Building created in the hosted context makes the broker download and cache them (clauses
//! 5.13.2 and 5.13.3).

use crate::building_payloads::{BUILDING_SIMPLE_TERMS, building_simple_attributes};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, remote_context::RemoteContexts, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{http::operation::CreateEntity, model::Context};
use std::collections::BTreeMap;
use url::Url;

/// Serves at `path` on `remote` the `application/ld+json` document mapping the simple Building terms to
/// `https://test.andromeda/<ns>/<term>`, and answers its URL.
pub(crate) async fn serve_building_terms(remote: &RemoteContexts, namespace: &Namespace, path: &str) -> Result<Url> {
    let mut terms = BTreeMap::new();
    for term in BUILDING_SIMPLE_TERMS {
        terms.insert(term, namespace.vocabulary_iri(term)?.into_inner());
    }
    let document = serde_json::to_string(&BTreeMap::from([("@context", terms)]))?;
    Ok(remote.serve(path, &document, Some("application/ld+json")).await?)
}

/// Posts the Building `urn:ngsi-ld:Building:<ns>-1` to `/entities/` with the hosted context at `location` (relative to
/// the broker) named in the `Link` header, then deletes it and its temporal evolution, each whatever the broker
/// answers.
pub(crate) async fn use_hosted_context(client: &BrokerClient, namespace: &Namespace, location: &IriRefBuf) -> Result<()> {
    let hosted = Url::parse(client.endpoint().base_url().as_str())?.join(location.as_str())?;
    let building = building_simple_attributes(namespace, 1)?;
    let writer = client.with_context(Context::from(IriBuf::new(hosted.to_string())?))?;
    wire::send_to(&writer, &CreateEntity::builder().entity(&building).build(), "entities/")
        .await
        .any_answer();
    client.entities().delete(&building.id).send().await.any_answer();
    client.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
