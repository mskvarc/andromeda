//! A server of the test process that the brokers call: the base of the notification sink and of the context source.
//!
//! The server listens on every interface of the runner container, on a port of its own, and the brokers reach it at
//! the callback host the configuration gives. It can start refusing: it then accepts each connection and closes it
//! unanswered, as a server that has not started yet does, until [`CallbackServer::start_serving`], which keeps the
//! port and records nothing of what came before.

use crate::config::{ConfigError, SuiteConfig};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr, TcpListener},
};
use tokio::{net::TcpListener as AsyncTcpListener, task::JoinHandle};
use url::{Host, Url};
use wiremock::{MockServer, Request};

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum CallbackError {
    /// The configuration names no host the brokers reach the test process by.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The listening socket could not be opened.
    #[error("the callback server could not listen")]
    Listen(#[source] io::Error),
    /// A URL of the server could not be built.
    #[error("{0} is not a URL")]
    Url(String, #[source] url::ParseError),
    /// The server was used before it served.
    #[error("the callback server still refuses connections")]
    Refusing,
}

/// What the server is doing.
#[derive(Debug)]
enum State {
    /// Accepting connections and closing them unanswered.
    Refusing {
        /// The socket the server takes over when it starts.
        listener: TcpListener,
        /// The task closing each accepted connection.
        refusal: JoinHandle<()>,
    },
    /// Answering and recording every request.
    Serving(MockServer),
}

/// A server the brokers call back.
#[derive(Debug)]
pub struct CallbackServer {
    /// The receiver.
    state: State,
    /// The base URL the brokers reach the server by, ending in `/`.
    base: Url,
}

impl CallbackServer {
    /// A server that serves from the start.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when no port can be opened or no callback host is known.
    pub async fn start() -> Result<CallbackServer, CallbackError> {
        let (listener, base) = listen()?;
        Ok(CallbackServer {
            state: State::Serving(MockServer::builder().listener(listener).start().await),
            base,
        })
    }

    /// A server that closes every connection unanswered until [`start_serving`](Self::start_serving).
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when no port can be opened or no callback host is known.
    pub fn refusing() -> Result<CallbackServer, CallbackError> {
        let (listener, base) = listen()?;
        let accepting = listener
            .try_clone()
            .and_then(|clone| {
                clone.set_nonblocking(true)?;
                AsyncTcpListener::from_std(clone)
            })
            .map_err(CallbackError::Listen)?;
        let refusal = tokio::spawn(async move {
            while let Ok((connection, _)) = accepting.accept().await {
                drop(connection);
            }
        });
        Ok(CallbackServer {
            state: State::Refusing { listener, refusal },
            base,
        })
    }

    /// The server answering from now on, on the same port, with nothing recorded.
    pub async fn start_serving(self) -> CallbackServer {
        let state = match self.state {
            State::Refusing { listener, refusal } => {
                refusal.abort();
                State::Serving(MockServer::builder().listener(listener).start().await)
            }
            serving @ State::Serving(_) => serving,
        };
        CallbackServer { state, base: self.base }
    }

    /// The URL of `path` on the server, as the brokers reach it.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError::Url`] when `path` breaks the URL.
    pub fn url(&self, path: &str) -> Result<Url, CallbackError> {
        self.base.join(path).map_err(|error| CallbackError::Url(path.to_owned(), error))
    }

    /// The server, once it serves.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError::Refusing`] before it serves.
    pub const fn server(&self) -> Result<&MockServer, CallbackError> {
        match &self.state {
            State::Serving(server) => Ok(server),
            State::Refusing { .. } => Err(CallbackError::Refusing),
        }
    }

    /// Every request the server received, in arrival order.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError::Refusing`] before it serves.
    pub async fn received(&self) -> Result<Vec<Request>, CallbackError> {
        Ok(self.server()?.received_requests().await.unwrap_or_default())
    }
}

/// Opens a port on every interface and the base URL the brokers reach it by.
fn listen() -> Result<(TcpListener, Url), CallbackError> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).map_err(CallbackError::Listen)?;
    let port = listener.local_addr().map_err(CallbackError::Listen)?.port();
    let config = SuiteConfig::get().map_err(|error| CallbackError::Config(error.clone()))?;
    let text = match config.callback_host()? {
        Host::Ipv6(address) => format!("http://[{address}]:{port}/"),
        host @ (Host::Domain(_) | Host::Ipv4(_)) => format!("http://{host}:{port}/"),
    };
    let base = Url::parse(&text).map_err(|error| CallbackError::Url(text, error))?;
    Ok((listener, base))
}
