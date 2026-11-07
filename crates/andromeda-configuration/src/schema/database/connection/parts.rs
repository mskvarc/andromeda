//! A PostgreSQL connection given by its parts, and the URL they make.

use derive_more::{AsRef, Display};
use schemars::JsonSchema;
use serde::Deserialize;
use std::{collections::BTreeMap, net::Ipv6Addr, num::NonZeroU16};
use thiserror::Error;
use url::{ParseError, Url};
use url_macro::url;

/// A literal `%` escaped for a URL.
const PERCENT_ESCAPE: &str = "%25";

/// The host a PostgreSQL server listens on: a DNS name, an IPv4 address, or
/// an IPv6 address written without brackets.
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "The host PostgreSQL listens on: a DNS name, an IPv4 address, or an IPv6 address without brackets.")]
pub struct ConnectionHost(String);

/// The PostgreSQL role the broker signs in as.
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "The PostgreSQL role the broker signs in as.")]
pub struct RoleName(String);

/// The password of the role, as written: the URL carries it percent-encoded.
#[derive(Debug, Clone, PartialEq, Eq, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "The role's password, as written. Characters a URL reserves need no escaping.")]
pub struct RolePassword(String);

/// The database the connection opens.
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "The database the connection opens.")]
pub struct DatabaseName(String);

/// The name of a libpq connection parameter, such as `sslmode`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Display, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "A libpq connection parameter name, such as `sslmode` or `application_name`.")]
pub struct ParameterName(String);

/// The value of a libpq connection parameter, as written.
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef, Deserialize, JsonSchema)]
#[as_ref(str)]
#[serde(transparent)]
#[schemars(description = "The parameter's value, as written.")]
pub struct ParameterValue(String);

/// A PostgreSQL connection given part by part, so each part can come from its
/// own source, such as a password from a secret, without escaping it into a
/// URL by hand.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The connection given by its parts, which the broker assembles into a URL.")]
pub struct ConnectionParts {
    /// The server's host.
    #[schemars(description = "The host PostgreSQL listens on: a DNS name, an IPv4 address, or an IPv6 address without brackets.")]
    pub host: ConnectionHost,
    /// The server's port; `None` is PostgreSQL's own default, 5432.
    #[serde(default)]
    #[schemars(description = "The port PostgreSQL listens on. Left out, it is 5432.")]
    pub port: Option<NonZeroU16>,
    /// The role the broker signs in as.
    #[schemars(description = "The PostgreSQL role the broker signs in as.")]
    pub user: RoleName,
    /// The role's password; `None` signs in without one, for trust or
    /// certificate authentication.
    #[serde(default)]
    #[schemars(description = "The role's password, as written. Left out, the broker signs in without one, as trust or certificate authentication allows.")]
    pub password: Option<RolePassword>,
    /// The database the connection opens.
    #[schemars(description = "The database the connection opens.")]
    pub database: DatabaseName,
    /// Further libpq connection parameters, carried as the URL's query.
    #[serde(default)]
    #[schemars(
        description = "Further libpq connection parameters, such as `sslmode = \"verify-full\"`, which the URL carries as its query string. Left out, there are none."
    )]
    pub parameters: BTreeMap<ParameterName, ParameterValue>,
}

/// The parts do not make a connection URL.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnectionPartsError {
    /// `host` is empty.
    #[error("`host` is empty")]
    EmptyHost,
    /// `host` is not a host name or address.
    #[error("`host` `{host}` is not a host name or address: {source}")]
    Host {
        /// The host as written.
        host: ConnectionHost,
        /// Why the URL refused it.
        source: ParseError,
    },
    /// `user` is empty.
    #[error("`user` is empty")]
    EmptyUser,
    /// `database` is empty.
    #[error("`database` is empty")]
    EmptyDatabase,
    /// The URL has no host to carry the port, the user, the password or the
    /// database: the only condition under which the URL refuses them.
    #[error("the connection URL has no host to carry its port, user, password or database")]
    NoHost,
}

impl TryFrom<ConnectionParts> for Url {
    type Error = ConnectionPartsError;

    fn try_from(parts: ConnectionParts) -> Result<Url, ConnectionPartsError> {
        let ConnectionParts {
            host,
            port,
            user,
            password,
            database,
            parameters,
        } = parts;
        if host.as_ref().is_empty() {
            return Err(ConnectionPartsError::EmptyHost);
        }
        if user.as_ref().is_empty() {
            return Err(ConnectionPartsError::EmptyUser);
        }
        if database.as_ref().is_empty() {
            return Err(ConnectionPartsError::EmptyDatabase);
        }
        let mut url = url!("postgres://localhost");
        let spelled_host = match host.as_ref().parse::<Ipv6Addr>() {
            Ok(address) => format!("[{address}]"),
            Err(_) => host.to_string(),
        };
        url.set_host(Some(&spelled_host))
            .map_err(|source| ConnectionPartsError::Host { host, source })?;
        url.set_port(port.map(NonZeroU16::get)).map_err(|()| ConnectionPartsError::NoHost)?;
        // The user and password setters escape every reserved character but
        // `%`, which they read as the start of an escape, so a literal one is
        // escaped first.
        url.set_username(&user.as_ref().replace('%', PERCENT_ESCAPE))
            .map_err(|()| ConnectionPartsError::NoHost)?;
        url.set_password(password.as_ref().map(|password| password.as_ref().replace('%', PERCENT_ESCAPE)).as_deref())
            .map_err(|()| ConnectionPartsError::NoHost)?;
        url.path_segments_mut()
            .map_err(|()| ConnectionPartsError::NoHost)?
            .clear()
            .push(database.as_ref());
        if !parameters.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(parameters.iter().map(|(name, value)| (name.as_ref(), value.as_ref())));
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::database::connection::parts::{ConnectionParts, ConnectionPartsError};
    use pretty_assertions::assert_eq;
    use url::Url;

    fn assembled(table: &str) -> Result<String, ConnectionPartsError> {
        Url::try_from(toml::from_str::<ConnectionParts>(table).expect("the parts read")).map(String::from)
    }

    #[test]
    fn the_parts_assemble_into_the_url_they_name() {
        assert_eq!(
            assembled("host = \"db.example\"\nport = 6543\nuser = \"broker\"\npassword = \"secret\"\ndatabase = \"andromeda\"\n"),
            Ok("postgres://broker:secret@db.example:6543/andromeda".to_owned())
        );
    }

    #[test]
    fn a_left_out_port_and_password_leave_the_url_without_them() {
        assert_eq!(
            assembled("host = \"db.example\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n"),
            Ok("postgres://broker@db.example/andromeda".to_owned())
        );
    }

    #[test]
    fn reserved_characters_in_the_user_password_and_database_are_percent_encoded() {
        assert_eq!(
            assembled("host = \"db.example\"\nuser = \"broker@ops%\"\npassword = \"p@ss:w/rd?#%41\"\ndatabase = \"a/b%\"\n"),
            Ok("postgres://broker%40ops%25:p%40ss%3Aw%2Frd%3F%23%2541@db.example/a%2Fb%25".to_owned())
        );
    }

    #[test]
    fn an_ipv6_host_is_bracketed_and_an_ipv4_host_is_kept() {
        assert_eq!(
            assembled("host = \"::1\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n"),
            Ok("postgres://broker@[::1]/andromeda".to_owned())
        );
        assert_eq!(
            assembled("host = \"192.0.2.7\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n"),
            Ok("postgres://broker@192.0.2.7/andromeda".to_owned())
        );
    }

    #[test]
    fn parameters_become_the_query_in_name_order() {
        assert_eq!(
            assembled(concat!(
                "host = \"db.example\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n",
                "[parameters]\nsslmode = \"verify-full\"\napplication_name = \"andromeda broker\"\n",
            )),
            Ok("postgres://broker@db.example/andromeda?application_name=andromeda+broker&sslmode=verify-full".to_owned())
        );
    }

    #[test]
    fn an_empty_host_user_or_database_is_refused() {
        assert_eq!(
            assembled("host = \"\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n"),
            Err(ConnectionPartsError::EmptyHost)
        );
        assert_eq!(
            assembled("host = \"db.example\"\nuser = \"\"\ndatabase = \"andromeda\"\n"),
            Err(ConnectionPartsError::EmptyUser)
        );
        assert_eq!(
            assembled("host = \"db.example\"\nuser = \"broker\"\ndatabase = \"\"\n"),
            Err(ConnectionPartsError::EmptyDatabase)
        );
    }

    #[test]
    fn a_host_with_a_character_no_host_may_hold_is_refused() {
        let refused = assembled("host = \"db example\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n");
        assert!(matches!(refused, Err(ConnectionPartsError::Host { .. })), "{refused:?}");
    }

    #[test]
    fn a_zero_port_is_refused_as_it_is_read() {
        assert!(toml::from_str::<ConnectionParts>("host = \"db.example\"\nport = 0\nuser = \"broker\"\ndatabase = \"andromeda\"\n").is_err());
    }
}
