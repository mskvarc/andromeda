//! One case of the interoperability suite: its namespace, the `@context` it speaks, the entities and registrations it
//! puts on the five brokers, and the reads it makes of them.
//!
//! Every request a case sends carries the case's `@context`: the vocabulary stored on b1 for the user-context variant
//! (the other brokers resolve it by the URL b1 serves it at), none for the default-context variant. The state the case
//! creates is removed on every broker when the case ends, whether it passes, fails or panics.

use crate::{
    composition::{attribute_names, read_back},
    node::Node,
    parking::{Kind, Sample},
    payload::Payload,
    topology::Link,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::{Cleanup, Removal},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::{ClientError, Received, ReqwestError},
    http::{
        ContextDelivery,
        Retrieved,
        UncheckedDocument,
        headers::Accept,
        operation::{QueryMethod, RegisterContextSource},
    },
    model::{Entity, EntityId, Input, Output},
    query::{Locality, SelectorType},
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    result,
};
use strum::IntoEnumIterator;

/// Which `@context` a case speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextChoice {
    /// The core `@context` alone: every term expands under the default vocabulary.
    Default,
    /// A vocabulary stored on b1 that maps every term of the case to an IRI of the case's namespace.
    User,
}

/// What a retrieval answered: the entity with the head of the response, or the error response.
type Read = result::Result<Received<Retrieved<Entity<Output>>>, ClientError<ReqwestError>>;

/// Runs the case twice, once per `@context`: a test named `$default_context` and a test named `$user_context`, each
/// calling the async function `$run` with its choice. The tests document what the case pins and the clauses it
/// exercises.
macro_rules! both_contexts {
    ($run:ident, $default_context:ident, $user_context:ident, pins = $pins:literal, clause = $clause:literal $(,)?) => {
        #[doc = concat!("Pins: ", $pins, " with the default context\nClause: ", $clause)]
        #[::googletest::gtest]
        #[::tokio::test]
        async fn $default_context() -> ::googletest::Result<()> {
            $run($crate::scenario::ContextChoice::Default).await
        }

        #[doc = concat!("Pins: ", $pins, " with a user context\nClause: ", $clause)]
        #[::googletest::gtest]
        #[::tokio::test]
        async fn $user_context() -> ::googletest::Result<()> {
            $run($crate::scenario::ContextChoice::User).await
        }
    };
}

/// A running case.
pub(crate) struct Scenario {
    /// The token of the case.
    namespace: Namespace,
    /// The vocabulary the case speaks, none for the default context.
    vocabulary: Option<Vocabulary>,
    /// How many registrations the case has made, the number of the next one.
    registrations: u32,
    /// What the case created on each broker, removed when the scenario is dropped.
    cleanups: BTreeMap<Node, Cleanup>,
}

impl Scenario {
    /// Starts a case speaking `choice`: stores the vocabulary of the user context on b1.
    pub(crate) async fn start(choice: ContextChoice) -> Result<Scenario> {
        let namespace = Namespace::fresh();
        let vocabulary = match choice {
            ContextChoice::Default => None,
            ContextChoice::User => Some(Scenario::store_vocabulary(&namespace).await?),
        };
        Ok(Scenario {
            namespace,
            vocabulary,
            registrations: 0,
            cleanups: BTreeMap::new(),
        })
    }

    /// Stores on b1 the vocabulary mapping the types and attribute names of the data files to the namespace. The core
    /// term `location` keeps its meaning.
    async fn store_vocabulary(namespace: &Namespace) -> Result<Vocabulary> {
        let parking = namespace.suffixed(Kind::Parking.base());
        let vehicle = namespace.suffixed(Kind::Vehicle.base());
        let terms = [
            parking.as_str(),
            vehicle.as_str(),
            "name",
            "availableSpotsNumber",
            "totalSpotsNumber",
            "reliability",
            "brandName",
            "speed",
            "source",
            "isParked",
        ];
        Ok(Vocabulary::register(&broker::client(Node::B1.broker())?, namespace, &terms).await?)
    }

    /// The namespace of the case.
    pub(crate) const fn namespace(&self) -> &Namespace {
        &self.namespace
    }

    /// A client of `node` for requests with a body, delivering the case's `@context` in the body.
    fn writer(&self, node: Node) -> Result<BrokerClient> {
        let client = broker::client(node.broker())?;
        match &self.vocabulary {
            None => Ok(client),
            Some(vocabulary) => Ok(client
                .with_context(vocabulary.context_list())?
                .with_context_delivery(ContextDelivery::LdJsonBody)),
        }
    }

    /// A client of `node` for requests without a body, delivering the case's `@context` in a `Link` header.
    fn reader(&self, node: Node) -> Result<BrokerClient> {
        let client = broker::client(node.broker())?;
        match &self.vocabulary {
            None => Ok(client),
            Some(vocabulary) => Ok(client.with_context(vocabulary.context())?),
        }
    }

    /// What the case created on `node`.
    fn cleanup(&mut self, node: Node) -> Result<&mut Cleanup> {
        match self.cleanups.entry(node) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => Ok(entry.insert(Cleanup::of(&broker::client(node.broker())?))),
        }
    }

    /// Has every broker delete the entity `id` locally when the case ends.
    pub(crate) fn track(&mut self, id: &EntityId) -> Result<()> {
        for node in Node::iter() {
            self.cleanup(node)?.remove(Removal::Entity(id.clone())); // clone: the cleanup outlives the case's values
        }
        Ok(())
    }

    /// Registers the data file `payload` on `hub`, pointing at `spoke`, and requires 201.
    ///
    /// The `mode` of the file is written into the document sent, since the model refuses an auxiliary registration
    /// that names no operations.
    pub(crate) async fn register(&mut self, hub: Node, payload: Payload, spoke: Node) -> Result<()> {
        self.registrations += 1;
        let endpoint = broker::peer(spoke.peer())?;
        let registration = payload.registration(&self.namespace, self.registrations, &endpoint)?;
        if let Some(id) = registration.id() {
            self.cleanup(hub)?.remove(Removal::Registration(id.clone())); // clone: the cleanup outlives the registration
        }
        let writer = self.writer(hub)?;
        let document = UncheckedDocument::try_from(registration.render())?.set_text("mode", &payload.mode().to_string());
        let request = writer.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
        let answer = writer.send_unchecked(request).await;
        verify_that!(&answer, answered(StatusCode::CREATED))
    }

    /// Registers each of `links` in order.
    pub(crate) async fn register_all(&mut self, links: &[Link]) -> Result<()> {
        for (hub, payload, spoke) in links {
            self.register(*hub, *payload, *spoke).await?;
        }
        Ok(())
    }

    /// Creates the entity of `sample` on `node` alone (`local=true`), requires 201 and answers the entity.
    pub(crate) async fn seed(&mut self, node: Node, sample: Sample) -> Result<Entity<Input>> {
        let entity = sample.entity(&self.namespace)?;
        self.cleanup(node)?.remove(Removal::Entity(entity.id.clone())); // clone: the cleanup outlives the entity
        let created = self
            .writer(node)?
            .with_responses()
            .entities()
            .create(&entity)
            .locality(Locality::Local)
            .send()
            .await;
        verify_that!(&created, answered(StatusCode::CREATED))?;
        Ok(entity)
    }

    /// Creates `entity` through `node` with the registrations applying, and requires 201. Every broker deletes the
    /// entity when the case ends.
    pub(crate) async fn create(&mut self, node: Node, entity: &Entity<Input>) -> Result<()> {
        self.track(&entity.id)?;
        let created = self.writer(node)?.with_responses().entities().create(entity).send().await;
        verify_that!(&created, answered(StatusCode::CREATED))
    }

    /// Retrieves the entity `id` from `node`, naming `kind` as its type when given, with the attributes `attrs` when
    /// any.
    async fn get(&self, node: Node, id: &EntityId, kind: Option<Kind>, attrs: &[&str], locality: Locality) -> Result<Read> {
        let reader = self.reader(node)?.with_accept(Accept::any()).with_responses();
        let entity_type = kind.map(|kind| self.namespace.suffixed(kind.base()));
        Ok(reader
            .entities()
            .retrieve(id)
            .maybe_entity_type(entity_type.as_deref())
            .attrs(attribute_names(attrs)?)
            .locality(locality)
            .send()
            .await)
    }

    /// The entity of a retrieval that required 200.
    fn found(read: Read) -> Result<Entity<Input>> {
        verify_that!(&read, answered(StatusCode::OK))?;
        read_back(read?.into_outcome().into_body())
    }

    /// Retrieves the entity `id` from `node` alone (`local=true`), requiring 200.
    pub(crate) async fn read_local(&self, node: Node, id: &EntityId) -> Result<Entity<Input>> {
        Scenario::found(self.get(node, id, None, &[], Locality::Local).await?)
    }

    /// Requires `node` to answer 404 to the retrieval of `id` from itself alone (`local=true`).
    pub(crate) async fn expect_absent(&self, node: Node, id: &EntityId) -> Result<()> {
        let read = self.get(node, id, None, &[], Locality::Local).await?;
        verify_that!(&read, answered(StatusCode::NOT_FOUND))
    }

    /// Retrieves the entity `id` of `kind` through `node` with the registrations applying, requiring 200.
    pub(crate) async fn retrieve(&self, node: Node, id: &EntityId, kind: Kind) -> Result<Entity<Input>> {
        Scenario::found(self.get(node, id, Some(kind), &[], Locality::default()).await?)
    }

    /// As [`retrieve`](Scenario::retrieve), asking for the attributes `attrs` only.
    pub(crate) async fn retrieve_attributes(&self, node: Node, id: &EntityId, kind: Kind, attrs: &[&str]) -> Result<Entity<Input>> {
        Scenario::found(self.get(node, id, Some(kind), attrs, Locality::default()).await?)
    }

    /// Retrieves the entity `id` of `kind` from `node` alone (`local=true`), requiring 200.
    pub(crate) async fn retrieve_local(&self, node: Node, id: &EntityId, kind: Kind) -> Result<Entity<Input>> {
        Scenario::found(self.get(node, id, Some(kind), &[], Locality::Local).await?)
    }

    /// Queries `node` for the entities of `kinds` with the registrations applying, asking for the attributes `attrs`
    /// only when any, through `method`, requiring 200.
    pub(crate) async fn query(&self, node: Node, kinds: &[Kind], attrs: &[&str], method: QueryMethod) -> Result<Vec<Entity<Input>>> {
        let types = kinds.iter().map(|kind| self.namespace.suffixed(kind.base())).collect::<Vec<_>>().join(",");
        let reader = self.reader(node)?.with_accept(Accept::any()).with_responses();
        let page = reader
            .entities()
            .query()
            .entity_type(types.parse::<SelectorType>()?)
            .attrs(attribute_names(attrs)?)
            .method(method)
            .send()
            .await;
        verify_that!(&page, answered(StatusCode::OK))?;
        page?.into_outcome().into_items().into_iter().map(read_back).collect()
    }
}
