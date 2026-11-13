//! What the database reports to `/health`.
//!
//! The probe lives beside the pool rather than beside the word "healthy":
//! `andromeda-health` declares the port and knows nothing about PostgreSQL, and
//! this is the only place that can answer for it.
//!
//! Readiness asks whether the database answers, not whether the platform pool
//! has a connection to spare. The ping runs over a pool of the indicator's own
//! and gives up after [`PING_BOUND`], so a platform pool whose every
//! connection is busy neither delays the ping nor fails it. The platform
//! pool's state is reported beside the ping, where its saturation shows.

use andromeda_health::{
    component::{ComponentDetail, ComponentName},
    details::ComponentDetails,
    indicator::HealthIndicator,
    postgresql_details::PostgreSqlDetails,
    status::HealthStatus,
};
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_postgresql_seaquery::{literal::int_literal, statement::Statement};
use async_trait::async_trait;
use futures::future::{BoxFuture, FutureExt, Shared};
use sea_query::Query;
use sqlx::PgPool;
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio::time::timeout;
use url::Url;

/// The connections the indicator opens to the platform database, beside the
/// platform pool.
pub const PROBE_CONNECTIONS: u32 = 1;

/// The longest a ping waits for the database, opening a connection included,
/// so a probe answers within about a second whatever the database does, well
/// inside the `database.acquire_timeout` a request waits for a connection.
const PING_BOUND: Duration = Duration::from_secs(1);

/// Why a ping did not succeed.
#[derive(Debug, thiserror::Error)]
enum PingFailure {
    /// No answer arrived within the bound: the server cannot be reached,
    /// refused every connection until the bound passed, or answers too slowly.
    #[error("the database did not answer within {} ms", PING_BOUND.as_millis())]
    Unanswered,
    /// The database answered with an error, or the connection to it failed.
    #[error(transparent)]
    Failed(sqlx::Error),
}

/// What every check sharing one ping reads: the round trip, or why the ping
/// failed.
type PingOutcome = Result<Duration, Arc<PingFailure>>;

/// One ping, awaited by every check that joins it.
type SharedPing = Shared<BoxFuture<'static, PingOutcome>>;

/// The ping in flight and when it started.
struct InFlight {
    /// When the ping started.
    started: Instant,
    /// The ping.
    ping: SharedPing,
}

/// Health indicator for the PostgreSQL database backend.
///
/// Pings the platform database over its own pool and reports the platform
/// pool's statistics alongside the measured latency.
pub struct DatabaseHealthIndicator {
    /// The indicator's own pool of [`PROBE_CONNECTIONS`] to the platform
    /// database, which nothing else borrows from.
    probe: PgPool,
    /// The platform pool, whose state is reported and never borrowed from.
    platform: PgPool,
    /// The ping in flight. A check that starts while it runs awaits it
    /// instead of sending another, so a burst of probes holds the probe
    /// connection once rather than queueing for it past the bound.
    in_flight: Mutex<Option<InFlight>>,
}

impl DatabaseHealthIndicator {
    /// Report on the platform database `platform_url` names, pinged over a
    /// pool of the indicator's own, and on the platform pool `platform`.
    ///
    /// No connection opens before the first check.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error`] when `platform_url` is not a PostgreSQL
    /// connection URL.
    pub fn new(platform_url: &Url, platform: PgPool) -> Result<DatabaseHealthIndicator, sqlx::Error> {
        Ok(DatabaseHealthIndicator {
            probe: configure_pool_options(PROBE_CONNECTIONS, PING_BOUND).connect_lazy(platform_url.as_str())?,
            platform,
            in_flight: Mutex::new(None),
        })
    }

    /// The ping in flight, or a new one when none is.
    ///
    /// A ping that settled is never reused, so every answer is fresh. Nor is
    /// one that started a bound ago and is still unsettled: every check that
    /// awaited it went away before it settled, and its timer has run out, so
    /// joining it would read a failure the database never caused.
    fn joined_ping(&self) -> SharedPing {
        let mut in_flight = self.in_flight.lock().unwrap_or_else(PoisonError::into_inner);
        match in_flight.as_ref() {
            Some(running) if running.ping.peek().is_none() && running.started.elapsed() < PING_BOUND => running.ping.clone(),
            Some(_) | None => {
                let ping = ping(self.probe.clone()).boxed().shared();
                *in_flight = Some(InFlight {
                    started: Instant::now(),
                    ping: ping.clone(),
                });
                ping
            }
        }
    }
}

/// Ping the platform database over `probe`, giving up after [`PING_BOUND`].
async fn ping(probe: PgPool) -> PingOutcome {
    let started = Instant::now();
    // The selected constant is never read, only the success of the round trip.
    let answered = timeout(PING_BOUND, Query::select().expr(int_literal(1)).to_query().execute(&probe)).await;
    match answered {
        Ok(Ok(_answered)) => Ok(started.elapsed()),
        // The probe pool waits for its connection, retrying a refused one, for
        // the same bound.
        Ok(Err(sqlx::Error::PoolTimedOut)) | Err(_) => Err(Arc::new(PingFailure::Unanswered)),
        Ok(Err(error)) => Err(Arc::new(PingFailure::Failed(error))),
    }
}

#[async_trait]
impl HealthIndicator for DatabaseHealthIndicator {
    fn name(&self) -> ComponentName {
        ComponentName::PostgreSql
    }

    async fn check(&self) -> ComponentDetail {
        match self.joined_ping().await {
            Ok(round_trip) => ComponentDetail {
                status: HealthStatus::Up,
                details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Reachable {
                    idle_connections: self.platform.num_idle(),
                    max_connections: self.platform.options().get_max_connections(),
                    // A latency past `u64::MAX` milliseconds is not representable and not
                    // reachable; reporting the cap keeps the detail a number either way.
                    ping_latency_ms: u64::try_from(round_trip.as_millis()).unwrap_or(u64::MAX),
                    pool_size: self.platform.size(),
                })),
            },
            Err(failure) => ComponentDetail {
                status: HealthStatus::Down,
                details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Unreachable { error: failure.to_string() })),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::health::{DatabaseHealthIndicator, PING_BOUND};
    use andromeda_health::{
        component::{ComponentDetail, ComponentName},
        details::ComponentDetails,
        indicator::HealthIndicator,
        postgresql_details::PostgreSqlDetails,
        status::HealthStatus,
    };
    use futures::future::join_all;
    use sqlx::PgPool;
    use std::{
        net::{Ipv4Addr, SocketAddr},
        time::{Duration, Instant},
    };
    use tokio::{net::TcpListener, time::sleep};
    use url::Url;

    /// What a check that went unanswered reports.
    fn unanswered() -> ComponentDetail {
        ComponentDetail {
            status: HealthStatus::Down,
            details: Some(ComponentDetails::PostgreSql(PostgreSqlDetails::Unreachable {
                error: "the database did not answer within 1000 ms".to_owned(),
            })),
        }
    }

    /// An indicator pinging the server at `address`, beside a platform pool
    /// that never connects.
    fn indicator_of(address: SocketAddr) -> DatabaseHealthIndicator {
        let url = Url::parse(&format!("postgres://andromeda@{address}/andromeda")).expect("a database URL");
        let platform = PgPool::connect_lazy(url.as_str()).expect("a lazy pool");
        DatabaseHealthIndicator::new(&url, platform).expect("an indicator")
    }

    /// A loopback address nothing listens on: a listener's, closed again.
    async fn refusing_address() -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("a loopback listener");
        listener.local_addr().expect("a bound address")
    }

    /// A loopback address that accepts every connection and never answers on
    /// it, as a server that has stopped responding does.
    async fn silent_address() -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("a loopback listener");
        let address = listener.local_addr().expect("a bound address");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((connection, _peer)) = listener.accept().await {
                held.push(connection);
            }
        });
        address
    }

    /// The margin a check is given past the bound before it counts as late:
    /// scheduling only, never another wait.
    const SCHEDULING_MARGIN: Duration = Duration::from_millis(500);

    #[tokio::test]
    async fn the_indicator_reports_under_the_postgresql_component_name() {
        pretty_assertions::assert_eq!(indicator_of(refusing_address().await).name(), ComponentName::PostgreSql);
    }

    #[tokio::test]
    async fn a_server_refusing_every_connection_is_down_within_the_bound() {
        let indicator = indicator_of(refusing_address().await);
        let started = Instant::now();
        pretty_assertions::assert_eq!(indicator.check().await, unanswered());
        assert!(started.elapsed() < PING_BOUND + SCHEDULING_MARGIN, "answered after {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn a_server_that_accepts_and_never_answers_is_down_within_the_bound() {
        let indicator = indicator_of(silent_address().await);
        let started = Instant::now();
        pretty_assertions::assert_eq!(indicator.check().await, unanswered());
        assert!(started.elapsed() < PING_BOUND + SCHEDULING_MARGIN, "answered after {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn checks_that_start_while_a_ping_runs_share_it_and_a_settled_ping_is_never_reused() {
        let indicator = indicator_of(silent_address().await);
        let first = indicator.joined_ping();
        let second = indicator.joined_ping();
        assert!(first.ptr_eq(&second), "a check starting while a ping runs joins it");
        assert!(first.await.is_err(), "nothing answers the ping");
        assert!(!second.ptr_eq(&indicator.joined_ping()), "a settled ping is replaced");
    }

    #[tokio::test]
    async fn a_ping_no_check_awaited_to_its_bound_is_replaced_rather_than_joined() {
        let indicator = indicator_of(silent_address().await);
        let abandoned = indicator.joined_ping();
        sleep(PING_BOUND).await;
        assert!(!abandoned.ptr_eq(&indicator.joined_ping()));
    }

    #[tokio::test]
    async fn a_burst_of_checks_against_a_silent_server_all_answer_within_the_bound() {
        let indicator = indicator_of(silent_address().await);
        let started = Instant::now();
        let details = join_all((0..32).map(|_| indicator.check())).await;
        assert!(details.iter().all(|detail| *detail == unanswered()), "{details:?}");
        assert!(started.elapsed() < PING_BOUND + SCHEDULING_MARGIN, "answered after {:?}", started.elapsed());
    }
}
