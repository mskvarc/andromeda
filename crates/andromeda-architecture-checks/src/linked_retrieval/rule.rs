//! Where each decision the engine owns may also be made: the engine itself,
//! and the few sources each rule names beside it.

/// The engine, exempt from every rule but the adapter registration.
pub(crate) const ENGINE: &str = "crates/andromeda-linked-retrieval/src/";

/// The clause 4.21 grammar, which defines the nested projection terms.
const PROJECTION_GRAMMAR: &str = "crates/andromeda-ngsi-ld-query/src/";

/// The sources that define the join modes or decode them from a request.
const JOIN_MODE_DEFINITIONS: [&str; 2] = ["crates/andromeda-ngsi-ld-representation/src/", "crates/andromeda-http-headers/src/"];

/// The two writers that spell an inline Linked Entity.
const INLINE_WRITERS: [&str; 2] = [
    "crates/andromeda-ngsi-ld-render/src/entity_body/",
    "crates/andromeda-ngsi-ld-render/src/temporal/inline_join.rs",
];

/// The one source that builds an `AttributeIriIndex`, by expanding keys.
const INDEX_OWNER: &str = "crates/andromeda-ngsi-ld-query/src/projection/attribute_iri_index.rs";

/// The registered adapters: where a source or a representation may be
/// implemented.
const REGISTERED_ADAPTERS: [&str; 4] = [
    "crates/andromeda-entity/src/query/core_linked_source.rs",
    "crates/andromeda-linked-retrieval/src/typed.rs",
    "crates/andromeda-temporal/src/linked/temporal_linked_source.rs",
    "crates/andromeda-ngsi-ld-render/src/temporal/temporal_evolutions.rs",
];

/// The decision a violation takes out of the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Reading a nested projection term.
    NestedTerm,
    /// Matching a pattern on a join mode.
    JoinModeMatch,
    /// Writing an inline member.
    InlineMember,
    /// Building an attribute index by hand.
    IndexBuild,
    /// Building an admission error outside its conversion.
    AdmissionError,
    /// Implementing an adapter in an unregistered file.
    Adapter,
    /// Naming a type or function after a private walk.
    PrivateWalk,
}

impl Rule {
    /// Whether a source at `path` may do what the rule forbids.
    pub(crate) fn permits(self, path: &str) -> bool {
        let in_engine = path.starts_with(ENGINE);
        match self {
            Rule::NestedTerm => in_engine || path.starts_with(PROJECTION_GRAMMAR),
            Rule::JoinModeMatch => in_engine || JOIN_MODE_DEFINITIONS.iter().any(|owner| path.starts_with(owner)),
            Rule::InlineMember => in_engine || INLINE_WRITERS.iter().any(|writer| path.starts_with(writer)),
            Rule::IndexBuild => path == INDEX_OWNER,
            Rule::AdmissionError | Rule::PrivateWalk => in_engine,
            Rule::Adapter => REGISTERED_ADAPTERS.contains(&path),
        }
    }
}
