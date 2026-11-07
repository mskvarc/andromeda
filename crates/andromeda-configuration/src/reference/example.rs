//! The configuration with every key that is not set by default set, which
//! gives the template and the documentation a value to show such a key at,
//! the database connections that have no default among them.

use crate::{
    broker_config::BrokerConfig,
    human_duration::HumanDuration,
    reference::ReferenceError,
    schema::{
        database::{connection::PostgresConnection, pooler::PoolerConfig},
        jsonld::PreloadedContext,
        source_alias::SourceAlias,
        source_extras::SourceExtras,
        source_identifier::SourceIdentifier,
    },
};
use iri_rs::iri;
use std::{num::NonZeroU32, path::PathBuf};
use url_macro::url;

/// The built-in defaults with every key that is not set by default set to an
/// example value.
///
/// # Errors
///
/// Returns [`ReferenceError::Example`] when the example
/// `[federation.identity.extras]` table does not read.
pub(crate) fn example_configuration() -> Result<BrokerConfig, ReferenceError> {
    let mut config = BrokerConfig::new(PostgresConnection::new(url!("postgres://andromeda:andromeda@localhost:5432/andromeda")));
    config.server.public_url = Some(url!("https://broker.example.org/"));
    config.tls.certificate = Some(PathBuf::from("/etc/andromeda/tls/tls.crt"));
    config.tls.private_key = Some(PathBuf::from("/etc/andromeda/tls/tls.key"));
    config.tls.client_authentication.ca = Some(PathBuf::from("/etc/andromeda/tls/clients.pem"));
    config.http.cors.preflight_lifetime = Some(HumanDuration::from_secs(600));
    // Two connections overlap a read with a write while the proxy does the
    // real backend pooling.
    config.database.pooler = Some(PoolerConfig {
        connection: PostgresConnection::new(url!("postgres://andromeda:andromeda@localhost:6432/andromeda")),
        max_connections_per_tenant: NonZeroU32::MIN.saturating_add(1),
    });
    config.jsonld.preloaded_contexts = vec![PreloadedContext {
        url: url!("https://example.org/ngsi-ld/context.jsonld"),
        path: PathBuf::from("contexts/example.jsonld"),
    }];
    config.temporal.storage.chunk_interval = Some(HumanDuration::from_days(7));
    config.temporal.storage.retention.delete_after = Some(HumanDuration::from_days(365));
    config.federation.identity.source_id = Some(SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:broker-a")));
    config.federation.identity.source_alias = Some(SourceAlias::from_static("broker-a"));
    config.federation.identity.extras = Some(toml::from_str::<SourceExtras>("operator = \"Example Operator\"\n")?);
    config.observability.log.file.directory = Some(PathBuf::from("/var/log/andromeda"));
    Ok(config)
}
