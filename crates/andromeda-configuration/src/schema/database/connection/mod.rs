//! Where a PostgreSQL server is and how the broker signs in to it, written as
//! a whole connection URL or as the URL's parts.
//!
//! The configuration holds the URL either spelling makes, so every consumer
//! reads one form. A connection has no built-in default: every deployment
//! names its own server.

pub mod parts;

use crate::schema::database::connection::parts::{ConnectionParts, ConnectionPartsError};
use derive_more::Into;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{self, MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::fmt;
use thiserror::Error;
use url::{ParseError, Url};

/// The PostgreSQL connection URL, read from either spelling and written as
/// the URL.
#[derive(Debug, Clone, PartialEq, Eq, Into, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "ConnectionSpelling", into = "Url")]
#[schemars(description = "A PostgreSQL connection, as a URL or as a table of its parts.")]
pub struct PostgresConnection(Url);

/// A connection URL string that does not parse as a URL.
#[derive(Debug, Error)]
#[error("the connection URL does not parse: {0}")]
pub struct UnparsableConnectionUrl(#[from] ParseError);

impl PostgresConnection {
    /// The connection a URL names.
    #[must_use]
    pub const fn new(url: Url) -> PostgresConnection {
        PostgresConnection(url)
    }

    /// The connection URL.
    #[must_use]
    pub const fn url(&self) -> &Url {
        &self.0
    }
}

impl TryFrom<ConnectionSpelling> for PostgresConnection {
    type Error = ConnectionPartsError;

    fn try_from(spelling: ConnectionSpelling) -> Result<PostgresConnection, ConnectionPartsError> {
        match spelling {
            ConnectionSpelling::Url(url) => Ok(PostgresConnection(url)),
            ConnectionSpelling::Parts(parts) => Url::try_from(parts).map(PostgresConnection),
        }
    }
}

/// How a configuration writes a connection: the whole URL as a string, or its
/// parts as a table.
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
#[schemars(untagged)]
#[schemars(description = "A PostgreSQL connection, as a URL or as a table of its parts.")]
pub enum ConnectionSpelling {
    /// The whole URL.
    #[schemars(
        description = "The whole connection URL, such as `\"postgres://andromeda:secret@db.example:5432/andromeda?sslmode=verify-full\"`. Characters a URL reserves in the user or password are percent-encoded."
    )]
    Url(Url),
    /// The URL's parts.
    #[schemars(
        description = "The connection's parts: `host`, `user` and `database`, and optionally `port`, `password` and a `parameters` table, each of which may come from its own environment variable."
    )]
    Parts(ConnectionParts),
}

// Not derived: an untagged derive buffers the value, which hides the URL's
// parse error behind "did not match any variant" and keeps the loader from
// reporting a misspelt part as an ignored key. A string reads as the URL and a
// table as the parts, each through the deserializer the loader wraps.
impl<'de> Deserialize<'de> for ConnectionSpelling {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ConnectionSpelling, D::Error> {
        deserializer.deserialize_any(SpellingVisitor)
    }
}

/// Reads a connection URL string or a table of its parts.
struct SpellingVisitor;

impl<'de> Visitor<'de> for SpellingVisitor {
    type Value = ConnectionSpelling;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a PostgreSQL connection URL, or a table of host, port, user, password, database and parameters")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<ConnectionSpelling, E> {
        Url::parse(value)
            .map(ConnectionSpelling::Url)
            .map_err(|error| E::custom(UnparsableConnectionUrl::from(error)))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ConnectionSpelling, A::Error> {
        ConnectionParts::deserialize(MapAccessDeserializer::new(map)).map(ConnectionSpelling::Parts)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::database::connection::PostgresConnection;
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Serialize)]
    struct Section {
        connection: PostgresConnection,
    }

    fn read(section: &str) -> Result<String, String> {
        toml::from_str::<Section>(section)
            .map(|section| section.connection.url().to_string())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn a_url_string_reads_as_that_url() {
        assert_eq!(
            read("connection = \"postgres://broker:secret@db.example:6543/andromeda?sslmode=require\"\n"),
            Ok("postgres://broker:secret@db.example:6543/andromeda?sslmode=require".to_owned())
        );
    }

    #[test]
    fn a_table_of_parts_reads_as_the_url_they_make() {
        assert_eq!(
            read("[connection]\nhost = \"db.example\"\nuser = \"broker\"\npassword = \"secret\"\ndatabase = \"andromeda\"\n"),
            Ok("postgres://broker:secret@db.example/andromeda".to_owned())
        );
    }

    #[test]
    fn a_url_that_does_not_parse_is_refused_with_the_parse_error() {
        let refused = read("connection = \"not a url\"\n").expect_err("not a URL");
        assert!(refused.contains("the connection URL does not parse: relative URL without a base"), "{refused}");
    }

    #[test]
    fn parts_that_make_no_url_are_refused_with_the_part_at_fault() {
        let refused = read("[connection]\nhost = \"db.example\"\nuser = \"\"\ndatabase = \"andromeda\"\n").expect_err("an empty user");
        assert!(refused.contains("`user` is empty"), "{refused}");
    }

    #[test]
    fn a_table_missing_a_required_part_is_refused() {
        let refused = read("[connection]\nhost = \"db.example\"\nuser = \"broker\"\n").expect_err("no database");
        assert!(refused.contains("database"), "{refused}");
    }

    #[test]
    fn either_spelling_writes_back_as_the_url() {
        let section: Section = toml::from_str("[connection]\nhost = \"db.example\"\nuser = \"broker\"\ndatabase = \"andromeda\"\n").expect("the parts read");
        assert_eq!(
            toml::to_string(&section).expect("writes"),
            "connection = \"postgres://broker@db.example/andromeda\"\n"
        );
    }
}
