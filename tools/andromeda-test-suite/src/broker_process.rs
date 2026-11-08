//! Stopping and starting a broker's process from a test, through the stack's signaller.
//!
//! The signaller is a service of the stack holding the container runtime's socket; it sends SIGTERM to a broker's
//! process, as an orchestrator does when it stops the broker, and starts the container again. Only the brokers whose
//! tests stop them are named, and nothing else uses them.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    eventually::{Poll, TimedOut, eventually},
};
use std::time::Duration;
use url::Url;

/// The longest a started broker takes to answer a request.
const SERVING_BOUND: Duration = Duration::from_secs(120);

/// Why a broker's process could not be signalled.
#[derive(Debug, thiserror::Error)]
pub enum BrokerProcessError {
    /// The stack runs no signaller.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The signaller or the broker could not be reached.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// The signaller refused the action.
    #[error("the signaller answered {status} for {action}")]
    Refused {
        /// The action asked of the signaller.
        action: SignalAction,
        /// The status the signaller answered.
        status: reqwest::StatusCode,
    },
    /// The signaller's address cannot name an action.
    #[error("the signaller's address cannot be extended with an action")]
    Unaddressable(#[source] url::ParseError),
    /// The started broker did not answer within the bound.
    #[error(transparent)]
    NotServing(#[from] TimedOut<String>),
}

/// What the signaller is asked to do to a broker's process.
#[derive(Debug, Clone, Copy, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum SignalAction {
    /// Send SIGTERM.
    Terminate,
    /// Start the stopped container.
    Start,
}

/// The process of one broker of the stack.
#[derive(Debug, Clone)]
pub struct BrokerProcess {
    /// The signaller's address.
    signaller: Url,
    /// The broker's own address.
    broker: Url,
    /// The name the signaller addresses the broker by: the role in lowercase kebab-case.
    name: String,
    /// The client every request goes through.
    http: reqwest::Client,
}

impl BrokerProcess {
    /// The process of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerProcessError`] when the stack runs no signaller or no such broker.
    pub fn of(role: BrokerRole) -> Result<BrokerProcess, BrokerProcessError> {
        let config = SuiteConfig::get().map_err(ConfigError::clone)?;
        let name = <&'static str>::from(role).to_lowercase().replace('_', "-");
        Ok(BrokerProcess {
            signaller: config.signaller()?.clone(),
            broker: config.broker(role)?.clone(),
            name,
            http: reqwest::Client::new(),
        })
    }

    /// Sends SIGTERM to the broker's process; the broker then drains and exits as it does when it is stopped.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerProcessError`] when the signaller cannot be reached or refuses.
    pub async fn terminate(&self) -> Result<(), BrokerProcessError> {
        self.signal(SignalAction::Terminate).await
    }

    /// Starts the broker again and waits until it answers requests.
    ///
    /// # Errors
    ///
    /// Returns [`BrokerProcessError`] when the signaller refuses or the broker does not answer within two minutes.
    pub async fn start(&self) -> Result<(), BrokerProcessError> {
        self.signal(SignalAction::Start).await?;
        eventually(SERVING_BOUND, || async {
            match self.http.get(self.broker.clone()).send().await {
                Ok(_) => Poll::Ready(()),
                Err(error) => Poll::Pending(error.to_string()),
            }
        })
        .await?;
        Ok(())
    }

    /// Asks the signaller to do `action` to the broker.
    async fn signal(&self, action: SignalAction) -> Result<(), BrokerProcessError> {
        let answered = self.http.get(self.address_of(action)?).send().await?.status();
        if answered.is_success() {
            Ok(())
        } else {
            Err(BrokerProcessError::Refused { action, status: answered })
        }
    }

    /// The address at which the signaller does `action` to the broker.
    fn address_of(&self, action: SignalAction) -> Result<Url, BrokerProcessError> {
        self.signaller
            .join(&format!("{action}-{}", self.name))
            .map_err(BrokerProcessError::Unaddressable)
    }
}
