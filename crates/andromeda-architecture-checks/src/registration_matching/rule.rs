//! Where each decision the engine owns may also be read: the engine itself,
//! the definitions the engine builds on, and the adapters registered beside
//! them.

/// The engine: the model modules that define registrations, subscriptions
/// and their entity selectors, and decide every match over them.
pub(crate) const ENGINE: [&str; 4] = [
    "crates/andromeda-ngsi-ld-model/src/entity_info.rs",
    "crates/andromeda-ngsi-ld-model/src/registration/",
    "crates/andromeda-ngsi-ld-model/src/registration_matching/",
    "crates/andromeda-ngsi-ld-model/src/subscription/",
];

/// The query crate, which defines the clause 4.9 `Pattern` and the
/// `IdentifierAxis` every selector reads its identifiers through.
const IDENTIFIER_DEFINITION: &str = "crates/andromeda-ngsi-ld-query/src/";

/// The decision a finding takes out of the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    /// Reading a registered or selected identifier pattern, or matching one.
    RegisteredIdentifier,
    /// Matching a registered or selected type expression.
    RegisteredType,
    /// Reading a registration's Scope.
    ScopeCoverage,
    /// Building a `csf` view of a registration.
    PropertyView,
    /// Comparing an `expiresAt` with the current instant.
    Expiry,
    /// Deciding standing on the subscription status alone.
    Standing,
    /// Building an identifier axis.
    SelectorAxis,
}

/// A source registered to read what `rule` guards, only to spell, persist or
/// push the rule down.
struct Adapter {
    /// The rule the source may break.
    rule: Rule,
    /// The workspace path of the source.
    path: &'static str,
}

/// The registered adapters, each reading only what it spells, persists or
/// pushes down.
const ADAPTERS: [Adapter; 7] = [
    // Writes a registered block's identifiers into the query forwarded to
    // that source.
    Adapter {
        rule: Rule::RegisteredIdentifier,
        path: "crates/andromeda-federation/src/forwarded_query/block_footprint.rs",
    },
    // Compiles a registered entry into the store query that probes for the
    // entities it selects.
    Adapter {
        rule: Rule::RegisteredIdentifier,
        path: "crates/andromeda-broker-composition/src/registered_entity_probe.rs",
    },
    // Writes a registered entry's `idPattern` and its portable form to the
    // information rows.
    Adapter {
        rule: Rule::RegisteredIdentifier,
        path: "crates/andromeda-postgresql-context-store/src/csource_registration/information_rows.rs",
    },
    // Copies a registered entry's `idPattern` into its compacted
    // representation.
    Adapter {
        rule: Rule::RegisteredIdentifier,
        path: "crates/andromeda-registration/src/information_compaction.rs",
    },
    // Renders the queried identifier axis as the clause 5.12 registration
    // preselection in SQL.
    Adapter {
        rule: Rule::SelectorAxis,
        path: "crates/andromeda-postgresql-context-query/src/csr/entity_specification.rs",
    },
    // Derives the read-only `isActive` member (Table 5.2.12-2) from the stored
    // status.
    Adapter {
        rule: Rule::Standing,
        path: "crates/andromeda-postgresql-context-store/src/subscription_store/mapper.rs",
    },
    // Resolves the stored Scope under a registration update fragment.
    Adapter {
        rule: Rule::ScopeCoverage,
        path: "crates/andromeda-registration/src/resolved_update.rs",
    },
];

/// The registered adapters no pair in `adapted` names: entries whose source
/// no longer reads what their rule guards.
pub(crate) fn stale_adapters(adapted: &[(Rule, String)]) -> Vec<String> {
    ADAPTERS
        .iter()
        .filter(|adapter| !adapted.iter().any(|(rule, path)| *rule == adapter.rule && path == adapter.path))
        .map(|adapter| {
            format!(
                "  {}: registered as an adapter of {:?} but reads nothing that rule guards",
                adapter.path, adapter.rule
            )
        })
        .collect()
}

impl Rule {
    /// Whether a source at `path` may do what the rule forbids.
    pub(crate) fn permits(self, path: &str) -> bool {
        let in_engine = ENGINE.iter().any(|owner| path.starts_with(owner));
        let defines_identifiers = path.starts_with(IDENTIFIER_DEFINITION);
        let registered = ADAPTERS.iter().any(|adapter| adapter.rule == self && adapter.path == path);
        match self {
            Rule::RegisteredIdentifier | Rule::SelectorAxis => in_engine || defines_identifiers || registered,
            Rule::RegisteredType | Rule::ScopeCoverage | Rule::PropertyView | Rule::Expiry | Rule::Standing => in_engine || registered,
        }
    }
}
