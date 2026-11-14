//! Where each decision the engine owns may also be made: the engine itself,
//! and the few sources each rule names beside it.

/// The engine, exempt from every rule but the adapter registration.
pub(crate) const ENGINE: &str = "crates/andromeda-linked-retrieval/src/";

/// The clause 4.21 grammar, which defines the nested projection terms.
const PROJECTION_GRAMMAR: &str = "crates/andromeda-ngsi-ld-query/src/";

/// The source that defines the join modes.
const JOIN_MODE_DEFINITION: &str = "crates/andromeda-ngsi-ld-representation/src/";

/// The writer that spells an inline Linked Entity, whichever kind of entity it
/// nests: an Entity, or the Temporal Evolution of one.
const INLINE_WRITER: &str = "crates/andromeda-ngsi-ld-render/src/entity_body/";

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

/// The operations whose Linked Entity rule admits every hop, each with the
/// source of the one pipeline that runs it.
const OPEN_OPERATIONS: [(&str, &str); 2] = [
    ("Subscription", "crates/andromeda-subscription/src/"),
    ("QueryRegistrations", "crates/andromeda-registration/src/"),
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
    /// Implementing the `q` hop admission port outside the engine.
    HopAdmission,
    /// Naming an operation that admits every hop outside its pipeline.
    OpenOperation {
        /// The source of the one pipeline that runs the operation.
        pipeline: &'static str,
    },
}

impl Rule {
    /// The rule naming `variant` of `LinkedOperation` breaks, when that
    /// operation admits every hop.
    pub(crate) fn open_operation(variant: &str) -> Option<Rule> {
        OPEN_OPERATIONS
            .iter()
            .find(|(name, _)| *name == variant)
            .map(|(_, pipeline)| Rule::OpenOperation { pipeline })
    }

    /// Whether a source at `path` may do what the rule forbids.
    pub(crate) fn permits(self, path: &str) -> bool {
        let in_engine = path.starts_with(ENGINE);
        match self {
            Rule::NestedTerm => in_engine || path.starts_with(PROJECTION_GRAMMAR),
            Rule::JoinModeMatch => in_engine || path.starts_with(JOIN_MODE_DEFINITION),
            Rule::InlineMember => in_engine || path.starts_with(INLINE_WRITER),
            Rule::IndexBuild => path == INDEX_OWNER,
            Rule::AdmissionError | Rule::PrivateWalk | Rule::HopAdmission => in_engine,
            Rule::OpenOperation { pipeline } => in_engine || path.starts_with(pipeline),
            Rule::Adapter => REGISTERED_ADAPTERS.contains(&path),
        }
    }
}
