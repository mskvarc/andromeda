//! The static registry that maps integration-test stacks to their compose file,
//! profile, environment, and owned suites. Every other module is a thin driver
//! over this table.

use clap::ValueEnum;

/// Environment variable naming the broker API base a suite talks to.
pub(crate) const BROKER_URL_ENV: &str = "ANDROMEDA_TEST_BROKER_URL";
/// Environment variable naming the trusted-edge broker's API base.
pub(crate) const TRUSTED_PROXY_BROKER_URL_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";
/// Environment variable naming the PostgreSQL control/tenant endpoint.
pub(crate) const DATABASE_URL_ENV: &str = "ANDROMEDA_TEST_DATABASE_URL";
/// Environment variable naming the conformance broker's API base.
pub(crate) const CONFORMANCE_BROKER_URL_ENV: &str = "ANDROMEDA_TEST_CONFORMANCE_BROKER_URL";
/// Environment variable naming the HTTP/3 authority the client dials.
pub(crate) const HTTP3_AUTHORITY_ENV: &str = "ANDROMEDA_TEST_HTTP3_AUTHORITY";
/// Environment variable naming the HTTP/3 leaf certificate the client trusts.
pub(crate) const HTTP3_CERT_ENV: &str = "ANDROMEDA_TEST_HTTP3_CERT";
/// Environment variable naming the second pooled replica broker's API base.
pub(crate) const BROKER_REPLICA_URL_ENV: &str = "ANDROMEDA_TEST_BROKER_REPLICA_URL";
/// Environment variable naming the wide-cache pooled broker's API base.
pub(crate) const BROKER_WIDE_URL_ENV: &str = "ANDROMEDA_TEST_BROKER_WIDE_URL";
/// Environment variable naming the un-pooled baseline broker's API base.
pub(crate) const BROKER_DIRECT_URL_ENV: &str = "ANDROMEDA_TEST_BROKER_DIRECT_URL";
/// Environment variable naming the `PgBouncer` admin-console endpoint (db `pgbouncer`).
pub(crate) const PGBOUNCER_ADMIN_URL_ENV: &str = "ANDROMEDA_TEST_PGBOUNCER_ADMIN_URL";

/// The `--test-threads` value a suite whose tests are isolated by per-test id
/// prefix runs at. Four keeps the pressure a 4-vCPU CI runner (which also hosts
/// PostgreSQL and the brokers) can absorb; a suite that needs a different degree
/// declares it with [`Suite::parallel`].
const PREFIX_ISOLATED_THREADS: u16 = 4;

/// One integration-test suite and the `--test-threads` value its isolation model
/// supports. A suite whose tests scope every resource by a per-test id prefix
/// (harness `TestEnvironment`, a shared advisory lock) runs in parallel; one that
/// resets the tenant (`SerializedEnvironment`, an exclusive lock), selects a bare
/// shared entity type, or reads tenant-wide state must run single-threaded.
pub(crate) struct Suite {
    /// The cargo `--test` target name.
    pub(crate) name: &'static str,
    /// The `--test-threads` value the suite's isolation allows.
    pub(crate) test_threads: u16,
}

impl Suite {
    /// A suite that must run single-threaded: it resets the tenant, selects a
    /// bare shared entity type, or depends on tenant-wide broker state.
    const fn serial(name: &'static str) -> Suite {
        Suite { name, test_threads: 1 }
    }

    /// A suite whose tests are isolated by per-test id prefix and so run in
    /// parallel at [`PREFIX_ISOLATED_THREADS`].
    const fn parallel(name: &'static str) -> Suite {
        Suite {
            name,
            test_threads: PREFIX_ISOLATED_THREADS,
        }
    }
}

/// The integration-test stacks the runner can drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum StackName {
    /// Shared infrastructure plus the two host-facing brokers on 1056/1057.
    Base,
    /// Four-broker distributed-operations conformance topology.
    Distributed,
    /// Five-broker federation acceptance topology.
    Federation,
    /// Snapshot acceptance topology (primary, replica, remote).
    Snapshot,
    /// Single broker serving HTTP/3 natively.
    Http3,
    /// Two-broker extensions topology (enabled and conformance).
    Extensions,
    /// Single broker whose request path runs through a `PgBouncer` transaction pooler.
    Pgbouncer,
}

/// One row of the registry: everything needed to bring a stack up and run its suites.
pub(crate) struct Stack {
    /// The stack's identity.
    pub(crate) name: StackName,
    /// Compose file path, relative to the repository root.
    pub(crate) compose_file: &'static str,
    /// The compose profile that gates the containerized brokers, if any.
    pub(crate) profile: Option<&'static str>,
    /// `ANDROMEDA_TEST_*` variables the suites read, minus the callback host.
    pub(crate) env: &'static [(&'static str, &'static str)],
    /// The suites this stack owns, each with the thread count its isolation allows.
    pub(crate) suites: &'static [Suite],
}

impl Stack {
    /// Borrow the registry entry for a stack name.
    pub(crate) fn get(name: StackName) -> &'static Stack {
        &STACKS[Self::index(name)]
    }

    /// The registry index of a stack name; total over every variant.
    const fn index(name: StackName) -> usize {
        match name {
            StackName::Base => 0,
            StackName::Distributed => 1,
            StackName::Federation => 2,
            StackName::Snapshot => 3,
            StackName::Http3 => 4,
            StackName::Extensions => 5,
            StackName::Pgbouncer => 6,
        }
    }
}

/// Locate the stack that owns a suite, returning the owning entry and the interned suite.
pub(crate) fn stack_for_suite(name: &str) -> Option<(&'static Stack, &'static Suite)> {
    STACKS
        .iter()
        .find_map(|stack| stack.suites.iter().find(|suite| suite.name == name).map(|suite| (stack, suite)))
}

/// The base tier's suites: every non-distributed suite plus the trusted-edge
/// regression pair. Suites marked [`Suite::parallel`] are isolated by per-test id
/// prefix and run multi-threaded; the rest reset the tenant, select a bare shared
/// type, or read tenant-wide state and stay single-threaded.
const BASE_SUITES: &[Suite] = &[
    Suite::parallel("aggregation"),
    Suite::serial("annex_c"),
    Suite::parallel("batch"),
    Suite::serial("cold_dictionary"),
    Suite::serial("common"),
    // Only two tests, one of which waits on a forwarding mock; parallelism is a
    // net loss here, so it stays single-threaded despite being prefix-isolated.
    Suite::serial("context_source_info"),
    Suite::serial("csf"),
    Suite::parallel("csr"),
    Suite::parallel("csr_notification"),
    Suite::parallel("csr_subscription"),
    Suite::parallel("current_projection"),
    Suite::serial("discovery"),
    Suite::serial("distop_attributes"),
    Suite::serial("distop_batch"),
    Suite::serial("distop_entity"),
    Suite::parallel("distop_entity_attributes"),
    Suite::serial("distop_temporal_entity"),
    Suite::serial("distop_temporal_entity_map"),
    Suite::serial("distop_types"),
    Suite::serial("durable_queue"),
    Suite::parallel("entity"),
    Suite::parallel("entity_attributes"),
    Suite::parallel("entity_map"),
    Suite::parallel("entity_order"),
    Suite::serial("expand_values"),
    Suite::parallel("expiration"),
    Suite::parallel("extension"),
    Suite::parallel("geometry"),
    Suite::parallel("info"),
    Suite::parallel("join"),
    Suite::serial("jsonld_context"),
    Suite::parallel("language"),
    Suite::parallel("list"),
    Suite::serial("multitenancy"),
    Suite::serial("name_dictionary"),
    Suite::parallel("notification"),
    Suite::parallel("notification_join"),
    Suite::parallel("pagination"),
    Suite::parallel("prefer"),
    Suite::parallel("profile"),
    Suite::serial("query_access_paths"),
    Suite::parallel("regression"),
    Suite::parallel("scope_query"),
    Suite::parallel("spatial"),
    Suite::parallel("spatial_3d"),
    Suite::parallel("subscription"),
    Suite::parallel("temporal_entity"),
    Suite::parallel("temporal_entity_attributes"),
    Suite::serial("temporal_semantics"),
    Suite::serial("tenant_management"),
    Suite::serial("tenant_schema"),
    Suite::parallel("unit_catalog"),
];

/// The complete stack registry, indexed by [`Stack::index`].
pub(crate) static STACKS: [Stack; 7] = [
    Stack {
        name: StackName::Base,
        compose_file: "docker/compose/test/base/docker-compose.yaml",
        profile: Some("broker"),
        env: &[
            (BROKER_URL_ENV, "http://127.0.0.1:1056/ngsi-ld/v1/"),
            (TRUSTED_PROXY_BROKER_URL_ENV, "http://127.0.0.1:1057/ngsi-ld/v1/"),
            (DATABASE_URL_ENV, "postgres://postgres:postgres@127.0.0.1:5432/postgres"),
        ],
        suites: BASE_SUITES,
    },
    Stack {
        name: StackName::Distributed,
        compose_file: "docker/compose/test/distributed/docker-compose.yaml",
        profile: None,
        // The distop topology reads DIST_* keys that already default to the
        // compose ports; only the front broker's database endpoint is pinned.
        env: &[(DATABASE_URL_ENV, "postgres://postgres:postgres@127.0.0.1:5442/broker_a")],
        suites: &[Suite::serial("distop")],
    },
    Stack {
        name: StackName::Federation,
        compose_file: "docker/compose/test/federation/docker-compose.yaml",
        profile: None,
        // The fed topology reads FED_* keys that already default to the compose ports.
        env: &[],
        suites: &[Suite::serial("fed")],
    },
    Stack {
        name: StackName::Snapshot,
        compose_file: "docker/compose/test/snapshot/docker-compose.yaml",
        profile: None,
        env: &[
            (BROKER_URL_ENV, "http://127.0.0.1:1081/ngsi-ld/v1/"),
            (DATABASE_URL_ENV, "postgres://postgres:postgres@127.0.0.1:5481/snapshot_primary"),
        ],
        suites: &[Suite::serial("snapshot")],
    },
    Stack {
        name: StackName::Http3,
        compose_file: "docker/compose/test/http3/docker-compose.yaml",
        profile: None,
        env: &[
            (BROKER_URL_ENV, "http://127.0.0.1:1056/ngsi-ld/v1/"),
            (HTTP3_AUTHORITY_ENV, "127.0.0.1:1056"),
            (HTTP3_CERT_ENV, "docker/compose/test/http3/certs/broker.crt"),
        ],
        suites: &[Suite::serial("http3")],
    },
    Stack {
        name: StackName::Extensions,
        compose_file: "docker/compose/test/extensions/docker-compose.yaml",
        profile: None,
        env: &[
            (BROKER_URL_ENV, "http://127.0.0.1:1091/ngsi-ld/v1/"),
            (CONFORMANCE_BROKER_URL_ENV, "http://127.0.0.1:1092/ngsi-ld/v1/"),
            (DATABASE_URL_ENV, "postgres://postgres:postgres@127.0.0.1:5491/postgres"),
        ],
        suites: &[Suite::serial("extensions")],
    },
    Stack {
        name: StackName::Pgbouncer,
        compose_file: "docker/compose/test/pgbouncer/docker-compose.yaml",
        profile: Some("broker"),
        env: &[
            (BROKER_URL_ENV, "http://127.0.0.1:1067/ngsi-ld/v1/"),
            (BROKER_REPLICA_URL_ENV, "http://127.0.0.1:1068/ngsi-ld/v1/"),
            (BROKER_WIDE_URL_ENV, "http://127.0.0.1:1069/ngsi-ld/v1/"),
            (BROKER_DIRECT_URL_ENV, "http://127.0.0.1:1070/ngsi-ld/v1/"),
            // The suites' own sampling, cleanup and verification connect straight
            // to PostgreSQL, bypassing the pooler, to read pg_stat_activity and
            // drop tenant databases.
            (DATABASE_URL_ENV, "postgres://postgres:postgres@127.0.0.1:5467/postgres"),
            // The pooler admin console (db `pgbouncer`) for SHOW POOLS/SERVERS and
            // PAUSE/RESUME/RECONNECT/KILL.
            (PGBOUNCER_ADMIN_URL_ENV, "postgres://postgres:postgres@127.0.0.1:6467/pgbouncer"),
        ],
        suites: &[
            Suite::serial("pgbouncer_scale"),
            Suite::serial("pgbouncer_semantics"),
            Suite::serial("pgbouncer_resilience"),
            Suite::serial("pgbouncer_dormant"),
        ],
    },
];

#[cfg(test)]
mod tests {
    use crate::stack::{STACKS, Stack, StackName, stack_for_suite};

    #[test]
    fn every_stack_name_indexes_its_own_registry_row() {
        for stack in &STACKS {
            assert_eq!(Stack::get(stack.name).name, stack.name);
        }
    }

    #[test]
    fn underscore_distop_suites_belong_to_base_and_bare_distop_to_distributed() {
        let (base, _) = stack_for_suite("distop_entity").expect("distop_entity is registered");
        assert_eq!(base.name, StackName::Base);
        let (distributed, _) = stack_for_suite("distop").expect("distop is registered");
        assert_eq!(distributed.name, StackName::Distributed);
    }

    #[test]
    fn singular_extension_is_base_and_plural_extensions_is_the_extensions_stack() {
        let (base, _) = stack_for_suite("extension").expect("extension is registered");
        assert_eq!(base.name, StackName::Base);
        let (extensions, _) = stack_for_suite("extensions").expect("extensions is registered");
        assert_eq!(extensions.name, StackName::Extensions);
    }

    #[test]
    fn an_unregistered_suite_resolves_to_no_stack() {
        assert!(stack_for_suite("does_not_exist").is_none());
    }
}
