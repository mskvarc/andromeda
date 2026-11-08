//! Holding a tenant database's whole pooler pool: sessions opened through the pooler, each running a transaction that
//! sleeps, so every request the broker sends to that database waits for a server connection.

use crate::{
    database_name::DatabaseName,
    eventually::{Poll, TimedOut, eventually},
    pooler_console::{PoolerConsole, PoolerConsoleError},
};
use andromeda_postgresql_seaquery::{catalog_function::pg_sleep, statement::Statement};
use sea_query::{Expr, Query};
use sqlx::{Connection, PgConnection};
use std::{fmt, time::Duration};
use tokio::task::{JoinError, JoinHandle};

/// How long the pooler is given to hand every holding session a server connection.
const SEIZE_BOUND: Duration = Duration::from_secs(15);

/// Why a pool could not be held.
#[derive(Debug, thiserror::Error)]
pub enum PoolHoldError {
    /// The pooler's console could not be read.
    #[error(transparent)]
    Console(#[from] PoolerConsoleError),
    /// The holding sessions did not all reach a server connection.
    #[error("the holding sessions did not seize the pool")]
    NotSeized(#[source] TimedOut<ActiveServers>),
    /// A holding session ended abnormally.
    #[error("a holding session ended abnormally")]
    Session(#[from] JoinError),
}

/// How many server connections of the held pool ran a client's transaction at the last look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveServers {
    /// The active server connections.
    pub active: i64,
    /// The sessions holding the pool.
    pub sessions: i64,
}

// Hand-written: the two counts read as one sentence of a failure message.
impl fmt::Display for ActiveServers {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} of {} holding sessions active", self.active, self.sessions)
    }
}

/// Sessions holding one database's pool until they end.
#[derive(Debug)]
pub struct PoolHold {
    /// The holding sessions, each answering how its sleep ended.
    sessions: Vec<JoinHandle<Result<(), sqlx::Error>>>,
}

impl PoolHold {
    /// Opens `sessions` connections to `database` through the pooler `console` belongs to, each sleeping `held_for`
    /// in a transaction of its own, and returns once the pooler shows all of them on a server connection.
    ///
    /// # Errors
    ///
    /// Returns [`PoolHoldError`] when the pooler cannot be read or the sessions do not all reach a server connection.
    pub async fn seize(console: &PoolerConsole, database: &DatabaseName, sessions: u8, held_for: Duration) -> Result<PoolHold, PoolHoldError> {
        let url = console.database_url(database);
        let seconds = held_for.as_secs_f64();
        let holding = (0..sessions)
            .map(|_| {
                let url = url.clone(); // clone: each spawned session moves its own copy of the URL
                tokio::spawn(async move {
                    let mut connection = PgConnection::connect(url.as_str()).await?;
                    let slept = Query::select()
                        .expr(pg_sleep(Expr::val(seconds)))
                        .to_owned()
                        .to_query()
                        .execute(&mut connection)
                        .await;
                    connection.close().await?;
                    slept.map(drop)
                })
            })
            .collect();
        let hold = PoolHold { sessions: holding };
        let awaited = i64::from(sessions);
        eventually(SEIZE_BOUND, || async {
            match console.pool_of(database).await {
                Ok(pool) => {
                    let active = pool.map_or(0, |pool| pool.servers_active);
                    if active >= awaited {
                        Poll::Ready(Ok(()))
                    } else {
                        Poll::Pending(ActiveServers { active, sessions: awaited })
                    }
                }
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await
        .map_err(PoolHoldError::NotSeized)??;
        Ok(hold)
    }

    /// Waits for every holding session to end, answering how each one's sleep ended: a session the pooler dropped
    /// ends in an error.
    ///
    /// # Errors
    ///
    /// Returns [`PoolHoldError::Session`] when a session ended abnormally.
    pub async fn release(self) -> Result<Vec<Result<(), sqlx::Error>>, PoolHoldError> {
        let mut ended = Vec::with_capacity(self.sessions.len());
        for session in self.sessions {
            ended.push(session.await?);
        }
        Ok(ended)
    }
}
