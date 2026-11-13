//! Holding relations of a broker's platform database locked, from one session connected straight to the broker's
//! server with its administrative login, until the test releases them: every statement of the broker that reads or
//! writes a held relation waits, holding its connection, until then.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use andromeda_postgresql_seaquery::{command::Command, relation_lock::RelationLock};
use sqlx::{Connection, PgConnection};
use tokio::{
    sync::oneshot,
    task::{JoinError, JoinHandle},
};

/// Why the relations could not be held.
#[derive(Debug, thiserror::Error)]
pub enum RelationHoldError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The platform database could not be reached, or a statement of the hold failed.
    #[error("the platform database could not be held")]
    Server(#[from] sqlx::Error),
    /// The holding session ended abnormally.
    #[error("the holding session ended abnormally")]
    Session(#[from] JoinError),
    /// The holding session ended before it took its lock, with no error to tell why.
    #[error("the holding session ended before it locked its relations")]
    NotLocked,
}

/// A session holding relations of one platform database until it is released.
#[derive(Debug)]
pub struct RelationHold {
    /// Ends the hold.
    release: oneshot::Sender<()>,
    /// The holding session, answering how its transaction ended.
    session: JoinHandle<Result<(), sqlx::Error>>,
}

impl RelationHold {
    /// Takes `lock` in the platform database of the broker of `role` and holds it until [`RelationHold::release`],
    /// returning once it is taken.
    ///
    /// # Errors
    ///
    /// Returns [`RelationHoldError`] when the stack names no such server, or the session cannot connect or take the
    /// lock.
    pub async fn seize(role: BrokerRole, lock: RelationLock) -> Result<RelationHold, RelationHoldError> {
        let url = SuiteConfig::get().map_err(Clone::clone)?.servers().server(role)?.clone(); // clone: the session task owns its URL
        let (locked, locked_signal) = oneshot::channel();
        let (release, released) = oneshot::channel::<()>();
        let session = tokio::spawn(async move {
            let mut connection = PgConnection::connect(url.as_str()).await?;
            let mut transaction = connection.begin().await?;
            lock.to_query().execute(&mut *transaction).await?;
            if locked.send(()).is_ok() {
                // Released or dropped, the hold ends the same way.
                released.await.unwrap_or_default();
            }
            transaction.rollback().await?;
            connection.close().await
        });
        match locked_signal.await {
            Ok(()) => Ok(RelationHold { release, session }),
            Err(_never_locked) => match session.await? {
                Ok(()) => Err(RelationHoldError::NotLocked),
                Err(error) => Err(error.into()),
            },
        }
    }

    /// Releases the relations and waits for the session to end.
    ///
    /// # Errors
    ///
    /// Returns [`RelationHoldError`] when the session ended abnormally or a statement of the hold failed.
    pub async fn release(self) -> Result<(), RelationHoldError> {
        // A session that already ended has nothing left to release.
        self.release.send(()).unwrap_or_default();
        Ok(self.session.await??)
    }
}
