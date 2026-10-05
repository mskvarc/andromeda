//! A representation and a source over plain test nodes, shared by the
//! traversal, encounter and plan tests.

use crate::{
    candidate::LinkedCandidate,
    edge::{LinkedEdge, RemoteEligibility},
    graph::LinkedGraph,
    keyword::Keyword,
    representation::LinkedRepresentation,
    source::{LevelRead, LinkedSource, RemoteReach, RemoteRead},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use enumset::EnumSet;
use indexmap::IndexMap;
use iri_rs::IriBuf;
use std::{
    collections::HashMap,
    future::{Future, ready},
    mem::take,
    num::NonZeroU32,
    sync::{Mutex, MutexGuard},
};

/// The identifier `urn:ngsi-ld:T:<raw>`.
pub(crate) fn id(raw: &str) -> Identifier {
    format!("urn:ngsi-ld:T:{raw}").parse().expect("valid URI")
}

/// An attribute name.
pub(crate) fn name(raw: &str) -> AttributeName {
    raw.parse().expect("valid attribute name")
}

/// One Relationship of a test node.
#[derive(Debug, Clone)]
pub(crate) struct Link {
    /// The attribute naming the target.
    pub(crate) attribute: AttributeName,
    /// The target.
    pub(crate) target: Identifier,
    /// Whether the Relationship carried an `objectType`.
    pub(crate) eligibility: RemoteEligibility,
}

/// One test entity.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Its identifier.
    pub(crate) id: Identifier,
    /// The Relationships its spelling shows.
    pub(crate) links: Vec<Link>,
    /// Its other attribute keys.
    pub(crate) properties: Vec<AttributeName>,
    /// Relationships only the level read reports, as a narrowed spelling hides
    /// them.
    pub(crate) hidden: Vec<Link>,
}

/// A node named `raw` whose Relationships are `(attribute, target)` pairs.
pub(crate) fn node(raw: &str, links: &[(&str, &str)]) -> Node {
    Node {
        id: id(raw),
        links: links
            .iter()
            .map(|(attribute, target)| Link {
                attribute: name(attribute),
                target: id(target),
                eligibility: RemoteEligibility::LocalOnly,
            })
            .collect(),
        properties: Vec::new(),
        hidden: Vec::new(),
    }
}

impl Node {
    /// The node with every Relationship annotated with an `objectType`.
    pub(crate) fn annotated(mut self) -> Node {
        for link in &mut self.links {
            link.eligibility = RemoteEligibility::Eligible;
        }
        self
    }

    /// The node carrying `properties` beside its Relationships.
    pub(crate) fn with_properties(mut self, properties: &[&str]) -> Node {
        self.properties = properties.iter().map(|property| name(property)).collect();
        self
    }

    /// The node showing no Relationship, while its level read reports them.
    pub(crate) fn hiding_its_links(mut self) -> Node {
        self.hidden = take(&mut self.links);
        self
    }

    /// The edges `links` name from this node.
    fn edges_of<'l>(&self, links: impl Iterator<Item = &'l Link>) -> impl Iterator<Item = LinkedEdge> {
        links.map(|link| LinkedEdge {
            from: self.id.clone(),
            attribute: link.attribute.clone(),
            target: link.target.clone(),
            eligibility: link.eligibility,
        })
    }
}

/// The representation of test nodes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Nodes;

impl LinkedRepresentation for Nodes {
    type Node = Node;

    fn identifier(node: &Node) -> Option<Identifier> {
        Some(node.id.clone())
    }

    fn edges(node: &Node, out: &mut Vec<LinkedEdge>) {
        out.extend(node.edges_of(node.links.iter()));
    }

    fn attribute_keys(node: &Node) -> impl Iterator<Item = &str> {
        node.links
            .iter()
            .map(|link| link.attribute.as_str())
            .chain(node.properties.iter().map(AttributeName::as_str))
    }

    fn keyword_members(_node: &Node) -> EnumSet<Keyword> {
        Keyword::Id | Keyword::Type
    }
}

/// What the remainder of the system answers for one identifier.
#[derive(Debug, Clone)]
pub(crate) enum RemoteAnswer {
    /// These nodes.
    Nodes(Vec<Node>),
    /// It cannot be reached.
    Unreachable,
}

/// Whether the test source consults the remainder of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Federation {
    /// It does.
    Federated,
    /// It does not.
    LocalOnly,
}

/// One call the traversal made into the test source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    /// A local read of these identifiers.
    Read(Vec<Identifier>),
    /// A remote fetch.
    Fetch {
        /// The identifier fetched.
        id: Identifier,
        /// The depth asked for.
        depth: u32,
        /// The identifiers passed on as held.
        held: Vec<Identifier>,
    },
}

/// A source over fixed local and remote nodes that records every call.
#[derive(Debug)]
pub(crate) struct FakeSource {
    /// The nodes the local store holds.
    local: HashMap<Identifier, Node>,
    /// What the remainder of the system answers.
    remote: HashMap<Identifier, RemoteAnswer>,
    /// Whether the remainder is consulted.
    federation: Federation,
    /// How far a remote holder resolves.
    reach: RemoteReach,
    /// Every call made.
    calls: Mutex<Vec<Call>>,
}

impl FakeSource {
    /// A source holding `local` and consulting nothing else.
    pub(crate) fn local(local: impl IntoIterator<Item = Node>) -> FakeSource {
        FakeSource {
            local: local.into_iter().map(|node| (node.id.clone(), node)).collect(),
            remote: HashMap::new(),
            federation: Federation::LocalOnly,
            reach: RemoteReach::Subgraph,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The source, consulting a remainder that answers `remote`.
    pub(crate) fn federated(mut self, remote: impl IntoIterator<Item = (&'static str, RemoteAnswer)>) -> FakeSource {
        self.remote = remote.into_iter().map(|(raw, answer)| (id(raw), answer)).collect();
        self.federation = Federation::Federated;
        self
    }

    /// The source, holding a remainder it never consults.
    pub(crate) const fn without_federation(mut self) -> FakeSource {
        self.federation = Federation::LocalOnly;
        self
    }

    /// The source, its remote holders answering for one entity only.
    pub(crate) const fn single_entity_holders(mut self) -> FakeSource {
        self.reach = RemoteReach::SingleEntity;
        self
    }

    /// Every call made so far.
    pub(crate) fn calls(&self) -> MutexGuard<'_, Vec<Call>> {
        self.calls.lock().expect("the call log is never poisoned")
    }

    /// The remote fetches made so far.
    pub(crate) fn fetches(&self) -> Vec<Call> {
        self.calls().iter().filter(|call| matches!(call, Call::Fetch { .. })).cloned().collect()
    }
}

impl LinkedSource for FakeSource {
    type Representation = Nodes;
    type Error = LinkedResolutionFailureKind;

    fn read_level(&self, candidates: &[LinkedCandidate]) -> impl Future<Output = Result<LevelRead<Node>, LinkedResolutionFailureKind>> + Send {
        self.calls().push(Call::Read(candidates.iter().map(|candidate| candidate.id.clone()).collect()));
        let mut read = LevelRead {
            nodes: IndexMap::new(),
            edges: Vec::new(),
        };
        for candidate in candidates {
            if let Some(node) = self.local.get(&candidate.id) {
                read.edges.extend(node.edges_of(node.links.iter().chain(node.hidden.iter())));
                read.nodes.insert(node.id.clone(), node.clone());
            }
        }
        ready(Ok(read))
    }

    fn fetch_remote(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> impl Future<Output = RemoteRead<Node>> + Send {
        self.calls().push(Call::Fetch {
            id: candidate.id.clone(),
            depth: depth.get(),
            held: held.to_vec(),
        });
        let read = match self.remote.get(&candidate.id) {
            Some(RemoteAnswer::Nodes(nodes)) => {
                let mut edges = Vec::new();
                for node in nodes {
                    Nodes::edges(node, &mut edges);
                }
                RemoteRead {
                    nodes: nodes.clone(),
                    edges,
                    failures: Vec::new(),
                }
            }
            Some(RemoteAnswer::Unreachable) => RemoteRead {
                nodes: Vec::new(),
                edges: Vec::new(),
                failures: vec![LinkedResolutionFailure {
                    id: candidate.id.clone(),
                    kind: LinkedResolutionFailureKind::Unreachable,
                    detail: "the holder cannot be reached".to_owned(),
                }],
            },
            None => RemoteRead {
                nodes: Vec::new(),
                edges: Vec::new(),
                failures: Vec::new(),
            },
        };
        ready(read)
    }

    fn federates(&self) -> bool {
        self.federation == Federation::Federated
    }

    fn remote_reach(&self, _candidate: &LinkedCandidate) -> impl Future<Output = RemoteReach> + Send {
        ready(self.reach)
    }
}

/// The graph a traversal of `depth` hops from `roots` would have built over
/// `nodes`, every node resolved.
pub(crate) fn graph(roots: &[Node], nodes: Vec<Node>, depth: u32) -> LinkedGraph<Node> {
    let mut edges = Vec::new();
    for entity in roots.iter().chain(nodes.iter()) {
        Nodes::edges(entity, &mut edges);
    }
    LinkedGraph {
        roots: roots.iter().map(|root| root.id.clone()).collect(),
        nodes: nodes.into_iter().map(|node| (node.id.clone(), node)).collect(),
        edges,
        failures: Vec::new(),
        depth: NonZeroU32::new(depth).expect("a positive depth"),
    }
}

/// The index mapping every key to `https://example.com/<key>`.
pub(crate) fn index(keys: &[&str]) -> AttributeIriIndex {
    let mut index = AttributeIriIndex::new();
    for key in keys {
        index.insert(name(key), iri(key));
    }
    index
}

/// `https://example.com/<key>`.
pub(crate) fn iri(key: &str) -> IriBuf {
    IriBuf::new(format!("https://example.com/{key}")).expect("valid IRI")
}

/// The projection term naming `key`, nesting `linked`.
pub(crate) fn term(key: &str, linked: Vec<ProjectionTerm>) -> ProjectionTerm {
    ProjectionTerm::Attribute { iri: iri(key), linked }
}
