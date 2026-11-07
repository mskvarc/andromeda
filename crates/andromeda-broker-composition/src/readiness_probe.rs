//! The `health` command: asking a running broker whether it is ready, on the
//! listener its configuration says answers readiness.
//!
//! In `separate-port` management mode that is `/readyz` on the management
//! listener, which keeps answering while the API is saturated or draining; in
//! `shared-port` mode it is `/readyz` at the root of the API listener; with the
//! management plane disabled it is the API's own health report. All three weigh
//! the same indicators and answer 503 when one is down.
//!
//! The probe dials the address the listener binds, which a certificate rarely
//! names, so it does not verify the certificate of a TLS listener. It carries no
//! credential and acts on the status code alone, the stance a Kubernetes HTTPS
//! probe takes.

use crate::{listener_tls::listener_scheme, outcome::ProcessExitOutcome};
use andromeda_configuration::schema::{
    listener_transport::ListenerTransport,
    management::{ManagementConfig, ManagementMode},
    server::ServerConfig,
};
use andromeda_http_api::api::API_ROOT_PATH;
use andromeda_http_extraction::api_root_scheme::ApiRootScheme;
use andromeda_http_health::routes::{HEALTH_PATH, READINESS_PATH};
use reqwest::{Client, StatusCode, redirect::Policy};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use thiserror::Error;
use url::{ParseError, Url};

/// The longest the probe waits for the broker's answer.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the broker could not be confirmed ready.
#[derive(Debug, Error)]
enum ReadinessProbeError {
    /// The configured listener address does not form a URL.
    #[error("the readiness probe address is not a valid url: {0}")]
    Url(#[from] ParseError),
    /// The HTTP client could not be built.
    #[error("the readiness probe client could not be built: {0}")]
    Client(#[source] reqwest::Error),
    /// No answer arrived: the listener refused the connection, the handshake
    /// failed or the timeout passed.
    #[error("the broker did not answer the readiness probe: {0}")]
    Request(#[source] reqwest::Error),
    /// The broker answered with a status other than success.
    #[error("the broker answered the readiness probe at {url} with {status}")]
    NotReady {
        /// The probed URL.
        url: Url,
        /// The status the broker answered with.
        status: StatusCode,
    },
}

/// Probe the broker that `server` and `management` describe, report the result
/// and return the process outcome.
pub(crate) async fn probe_reported(server: &ServerConfig, management: &ManagementConfig) -> ProcessExitOutcome {
    match probe(server, management).await {
        Ok(url) => {
            println!("Broker ready: {url}");
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::NotReady
        }
    }
}

/// Send one readiness request and return the probed URL when the broker
/// answered it with success.
///
/// # Errors
///
/// Returns a [`ReadinessProbeError`] when no URL or client can be built, when
/// no answer arrives, or when the answer is not a success.
async fn probe(server: &ServerConfig, management: &ManagementConfig) -> Result<Url, ReadinessProbeError> {
    let url = readiness_url(server, management)?;
    // A proxy from the environment would carry a loopback request off the host,
    // and a redirect would answer for another listener.
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(PROBE_TIMEOUT)
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(ReadinessProbeError::Client)?;
    let status = client.get(url.as_str()).send().await.map_err(ReadinessProbeError::Request)?.status();
    if status.is_success() {
        Ok(url)
    } else {
        Err(ReadinessProbeError::NotReady { url, status })
    }
}

/// The URL of the endpoint that answers readiness under the configured
/// management topology.
fn readiness_url(server: &ServerConfig, management: &ManagementConfig) -> Result<Url, ParseError> {
    match management.mode {
        ManagementMode::SeparatePort => listener_url(management.host, management.port, management.transport, READINESS_PATH),
        ManagementMode::SharedPort => listener_url(server.host, server.port, server.transport, READINESS_PATH),
        ManagementMode::Disabled => listener_url(server.host, server.port, server.transport, &format!("{API_ROOT_PATH}{HEALTH_PATH}")),
    }
}

/// The URL of `path` on the listener bound to `host` and `port` and speaking
/// `transport`, as a client on the same host reaches it.
fn listener_url(host: IpAddr, port: u16, transport: ListenerTransport, path: &str) -> Result<Url, ParseError> {
    let scheme = ApiRootScheme::from(listener_scheme(transport));
    let address = SocketAddr::new(reachable_host(host), port);
    Url::parse(&format!("{}://{address}{path}", scheme.as_scheme()))
}

/// The address a client on the same host reaches a listener bound to `host`
/// at: a wildcard bind answers on the loopback address of its family.
const fn reachable_host(host: IpAddr) -> IpAddr {
    match host {
        IpAddr::V4(address) if address.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(address) if address.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        IpAddr::V4(_) | IpAddr::V6(_) => host,
    }
}

#[cfg(test)]
mod tests {
    use crate::readiness_probe::{ReadinessProbeError, probe, readiness_url};
    use andromeda_configuration::schema::{
        listener_transport::ListenerTransport,
        management::{ManagementConfig, ManagementMode},
        server::ServerConfig,
    };
    use andromeda_health::{
        aggregate::Health,
        component::{ComponentDetail, ComponentName},
        indicator::HealthIndicator,
        status::HealthStatus,
    };
    use andromeda_http_health::routes::HealthRoutes;
    use async_trait::async_trait;
    use reqwest::StatusCode;
    use std::{
        error::Error,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
    };
    use tokio::net::TcpListener;

    /// An indicator that always reports the status it was built with.
    struct FixedIndicator {
        /// The status it always reports.
        status: HealthStatus,
    }

    #[async_trait]
    impl HealthIndicator for FixedIndicator {
        fn name(&self) -> ComponentName {
            ComponentName::PostgreSql
        }

        async fn check(&self) -> ComponentDetail {
            ComponentDetail {
                status: self.status,
                details: None,
            }
        }
    }

    /// A management configuration in `separate-port` mode on `port` of the IPv4
    /// wildcard address.
    fn separate_port_management(port: u16) -> ManagementConfig {
        ManagementConfig {
            port,
            ..ManagementConfig::default()
        }
    }

    /// Serve the management probes over an indicator reporting `status` on an
    /// ephemeral loopback port, and return that port.
    async fn serve_management_probes(status: HealthStatus) -> Result<u16, Box<dyn Error>> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        let router = HealthRoutes::new(Health::builder().with_indicator(FixedIndicator { status }).build()).into_management_router();
        tokio::spawn(async move { axum::serve(listener, router).await });
        Ok(port)
    }

    #[test]
    fn the_default_configuration_probes_readyz_on_the_management_port_over_loopback() -> Result<(), Box<dyn Error>> {
        let url = readiness_url(&ServerConfig::default(), &ManagementConfig::default())?;
        pretty_assertions::assert_eq!(url.as_str(), "http://127.0.0.1:1086/readyz");
        Ok(())
    }

    #[test]
    fn shared_port_mode_probes_readyz_on_the_api_listener_with_its_transport() -> Result<(), Box<dyn Error>> {
        let server = ServerConfig {
            host: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            port: 1443,
            transport: ListenerTransport::Tls,
            ..ServerConfig::default()
        };
        let management = ManagementConfig {
            mode: ManagementMode::SharedPort,
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(readiness_url(&server, &management)?.as_str(), "https://[::1]:1443/readyz");
        Ok(())
    }

    #[test]
    fn disabled_management_probes_the_api_health_report_on_the_bound_address() -> Result<(), Box<dyn Error>> {
        let server = ServerConfig {
            host: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)),
            port: 2000,
            ..ServerConfig::default()
        };
        let management = ManagementConfig {
            mode: ManagementMode::Disabled,
            ..ManagementConfig::default()
        };
        pretty_assertions::assert_eq!(readiness_url(&server, &management)?.as_str(), "http://10.0.0.5:2000/ngsi-ld/v1/health");
        Ok(())
    }

    #[tokio::test]
    async fn a_broker_whose_indicators_are_up_is_ready() -> Result<(), Box<dyn Error>> {
        let port = serve_management_probes(HealthStatus::Up).await?;
        let url = probe(&ServerConfig::default(), &separate_port_management(port)).await?;
        pretty_assertions::assert_eq!(url.port(), Some(port));
        Ok(())
    }

    #[tokio::test]
    async fn a_broker_with_a_down_indicator_is_not_ready_with_503() -> Result<(), Box<dyn Error>> {
        let port = serve_management_probes(HealthStatus::Down).await?;
        let outcome = probe(&ServerConfig::default(), &separate_port_management(port)).await;
        assert!(matches!(
            outcome,
            Err(ReadinessProbeError::NotReady {
                status: StatusCode::SERVICE_UNAVAILABLE,
                ..
            })
        ));
        Ok(())
    }
}
