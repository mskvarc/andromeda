//! The reference TLS-terminating deployment the clause 6.2 claim rests on.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2: "The API shall support HTTP over TLS
//! (also known as HTTPS - see IETF RFC 2818 [18]). TLS version 1.2 as defined
//! by IETF RFC 5246 [19] shall be supported. HTTP without TLS is not
//! recommended."
//!
//! The `andromeda` binary implements no TLS: it serves plaintext HTTP, and the
//! clause is met by the deployment described in `docs/tls-deployment.md` and
//! built by `docker/tls/`. That makes those two files load-bearing rather than
//! illustrative, and the three properties the claim actually depends on are
//! properties of their content:
//!
//! - the terminator offers TLS 1.2, and nothing the IETF has since deprecated;
//! - the terminator **replaces** the forwarded fields rather than appending to
//!   them, which is what makes `ANDROMEDA_HTTP_FORWARDED_HEADERS=trusted` safe;
//! - the broker publishes no port of its own, which is what makes the TLS
//!   requirement non-vacuous and the trust setting admissible at all.
//!
//! These assert those properties from the repository, so they run wherever the
//! test suite runs. They are not a substitute for standing the stack up: only a
//! live terminator proves the handshake, and `docs/tls-deployment.md` records
//! the command that does it.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use pretty_assertions::assert_eq;
use serde_norway::Value;
use std::{fs, mem::take, path::PathBuf};

/// The reference proxy configuration.
const NGINX_CONFIGURATION: &str = "docker/tls/nginx.conf";

/// The reference stack that runs a broker behind that proxy.
const TLS_COMPOSE_FILE: &str = "docker/tls/docker-compose.tls.yaml";

/// The two forwarded fields the broker reads, and the only two whose value a
/// client must never be able to influence at a trusted edge.
const PROTOCOL_FIELDS: [&str; 2] = ["Forwarded", "X-Forwarded-Proto"];

/// The nginx variables that carry those two fields as the client sent them.
///
/// A value built from one of these turns `proxy_set_header`'s replacement back
/// into an append, leaving a client-supplied element in the leftmost position --
/// the position the broker reads. `$http_host` is deliberately absent: it
/// carries the client's `Host`, which is the value the broker is meant to take
/// its authority from.
const CLIENT_SUPPLIED_PROTOCOL_VARIABLES: [&str; 3] = ["$http_forwarded", "$http_x_forwarded_proto", "$proxy_add_x_forwarded_proto"];

/// One nginx directive: its name and its arguments, with quoting removed.
struct NginxDirective {
    /// The directive's first token.
    name: String,
    /// Every token after the name, unquoted.
    arguments: Vec<String>,
}

/// Scan an nginx configuration into the directives it declares.
///
/// nginx configuration is a sequence of directives, each ended by `;` or by the
/// `{` that opens its block; a directive's first token is its name and the rest
/// are its arguments. Comments run to the end of a line, and a double-quoted
/// argument may contain the `;` that would otherwise end the directive -- which
/// `proxy_set_header Forwarded "proto=$scheme;host=$http_host";` does, so it is
/// read rather than ignored. That is the whole grammar this needs: enough to
/// assert on the directives the deployment claim rests on, without depending on
/// a parser crate that has been unmaintained since 2019.
fn nginx_directives(configuration: &str) -> Vec<NginxDirective> {
    let mut directives = Vec::new();
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut commented = false;
    for character in configuration.chars() {
        match character {
            '\n' if commented => commented = false,
            _ if commented => {}
            '#' if !quoted => commented = true,
            '"' => quoted = !quoted,
            ';' | '{' | '}' if !quoted => {
                if !token.is_empty() {
                    tokens.push(take(&mut token));
                }
                if let Some((name, arguments)) = tokens.split_first() {
                    directives.push(NginxDirective {
                        name: name.clone(), // clone: the directive owns its name while the token buffer is reused.
                        arguments: arguments.to_vec(),
                    });
                }
                tokens.clear();
            }
            _ if character.is_whitespace() && !quoted => {
                if !token.is_empty() {
                    tokens.push(take(&mut token));
                }
            }
            _ => token.push(character),
        }
    }
    directives
}

/// The arguments of every occurrence of one directive, in file order.
fn arguments_of<'directives>(directives: &'directives [NginxDirective], name: &str) -> Vec<&'directives [String]> {
    directives
        .iter()
        .filter(|directive| directive.name == name)
        .map(|directive| directive.arguments.as_slice())
        .collect()
}

/// Read one repository file the deployment claim rests on.
fn deployment_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    let content = fs::read_to_string(&path);
    assert!(content.is_ok(), "{} must be readable: {content:?}", path.display());
    content.expect("asserted to be readable above")
}

/// The service definitions of the reference TLS stack.
fn tls_stack_services() -> Value {
    let compose: Value = serde_norway::from_str(&deployment_file(TLS_COMPOSE_FILE)).expect("the TLS compose file must be valid YAML");
    compose.get("services").expect("a compose file declares services").clone() // clone: the test owns the services while the document is dropped.
}

/// Clause 6.2 names TLS 1.2 as the floor a deployment has to offer. IETF RFC
/// 8996 deprecates TLS 1.0 and 1.1, so offering either would meet the clause
/// while weakening the connection it is about.
#[test]
fn regression_the_reference_terminator_offers_the_tls_version_clause_6_2_requires() {
    let directives = nginx_directives(&deployment_file(NGINX_CONFIGURATION));
    let protocols = arguments_of(&directives, "ssl_protocols");
    assert_eq!(protocols.len(), 1, "the terminator pins its protocol floor exactly once");
    let offered = protocols.first().expect("asserted to be present above");

    assert!(
        offered.iter().any(|version| version == "TLSv1.2"),
        "clause 6.2 requires TLS 1.2 to be supported: {offered:?}"
    );
    for deprecated in ["TLSv1", "TLSv1.1", "SSLv2", "SSLv3"] {
        assert!(
            !offered.iter().any(|version| version == deprecated),
            "{deprecated} is deprecated by IETF RFC 8996 and must not be offered: {offered:?}"
        );
    }
}

/// The terminator holds a certificate and listens for TLS, which is the half of
/// clause 6.2 the binary does not implement.
#[test]
fn regression_the_reference_terminator_terminates_tls_rather_than_forwarding_it() {
    let directives = nginx_directives(&deployment_file(NGINX_CONFIGURATION));

    let listeners = arguments_of(&directives, "listen");
    assert!(
        listeners.iter().any(|arguments| arguments.iter().any(|argument| argument == "ssl")),
        "the terminator has a TLS listener: {listeners:?}"
    );
    for material in ["ssl_certificate", "ssl_certificate_key"] {
        assert_eq!(arguments_of(&directives, material).len(), 1, "the terminator names its {material} exactly once");
    }
}

/// Clause 6.2: "HTTP without TLS is not recommended." The broker cannot tell a
/// plaintext request that arrived through the terminator from one that bypassed
/// it, so sending clients to the TLS listener is the terminator's job.
#[test]
fn regression_the_reference_terminator_sends_plaintext_clients_to_the_tls_listener() {
    let directives = nginx_directives(&deployment_file(NGINX_CONFIGURATION));
    let redirects = arguments_of(&directives, "return");

    assert!(
        redirects
            .iter()
            .any(|arguments| arguments.first().is_some_and(|status| status == "301") && arguments.iter().any(|argument| argument.starts_with("https://"))),
        "the plaintext listener redirects to the TLS one: {redirects:?}"
    );
}

/// The requirement that makes `ANDROMEDA_HTTP_FORWARDED_HEADERS=trusted`
/// admissible: the terminator must set the two protocol-carrying fields to a
/// value of its own, never to one derived from what the client sent. nginx's
/// `proxy_set_header` replaces the field; a value built from `$http_forwarded`
/// or `$proxy_add_x_forwarded_proto` would carry the client's element into the
/// leftmost position, which is exactly the position the broker reads.
#[test]
fn regression_the_reference_terminator_replaces_the_forwarded_fields_it_sends() {
    let directives = nginx_directives(&deployment_file(NGINX_CONFIGURATION));
    let set_fields = arguments_of(&directives, "proxy_set_header");

    for field in PROTOCOL_FIELDS {
        let sent: Vec<&[String]> = set_fields
            .iter()
            .copied()
            .filter(|arguments| arguments.first().is_some_and(|name| name == field))
            .collect();
        assert_eq!(sent.len(), 1, "the terminator sets {field} exactly once: {set_fields:?}");
        let value = sent.first().expect("asserted to be present above");
        for argument in value.iter().skip(1) {
            for inherited in CLIENT_SUPPLIED_PROTOCOL_VARIABLES {
                assert!(
                    !argument.contains(inherited),
                    "{field} must not be built from {inherited}, which carries what the client sent: {argument}"
                );
            }
        }
        assert!(
            value.iter().skip(1).any(|argument| argument.contains("$scheme")),
            "{field} must carry the scheme the terminator itself terminated: {value:?}"
        );
    }
}

/// The broker takes the authority half of its origin from the request's own
/// `Host` field and reads no forwarded host, so a terminator that rewrote
/// `Host` to its upstream address would mint URLs naming the compose network.
///
/// The value has to be `$http_host`, the client's field verbatim. nginx's
/// `$host` drops the port, and a terminator listening anywhere but 443 would
/// then have the broker mint URLs naming port 443 -- durably, in the `@context`
/// URL clause 5.13 has it store.
#[test]
fn regression_the_reference_terminator_passes_the_client_host_through() {
    let directives = nginx_directives(&deployment_file(NGINX_CONFIGURATION));
    let host = arguments_of(&directives, "proxy_set_header")
        .into_iter()
        .find(|arguments| arguments.first().is_some_and(|name| name == "Host"))
        .expect("the terminator sets Host");

    assert_eq!(
        host.get(1).map(String::as_str),
        Some("$http_host"),
        "Host is passed through with its port intact: {host:?}"
    );
}

/// Trusting the forwarded fields is only admissible while nothing but the
/// terminator can set them, so the broker of the reference stack publishes no
/// host port. Publishing one would both make the TLS requirement vacuous and
/// hand any client the scheme of the broker's persisted `@context` URLs.
#[test]
fn regression_the_reference_tls_stack_publishes_only_the_terminator() {
    let services = tls_stack_services();

    for hidden in ["broker", "postgres"] {
        let service = services.get(hidden).unwrap_or_else(|| panic!("the TLS stack declares a {hidden} service"));
        assert!(service.get("ports").is_none(), "the {hidden} service must publish no host port");
    }
    let published = services
        .get("terminator")
        .and_then(|terminator| terminator.get("ports"))
        .and_then(Value::as_sequence)
        .expect("the terminator publishes its listeners");
    assert!(
        published.iter().filter_map(Value::as_str).any(|mapping| mapping.contains("8443")),
        "the terminator publishes its TLS listener: {published:?}"
    );
}

/// The broker of the reference stack is the trusted half of the deployment, and
/// it reads the reference proxy configuration this module asserts on -- read
/// only, so a running container cannot rewrite the file that makes the trust
/// safe.
#[test]
fn regression_the_reference_tls_stack_trusts_the_terminator_it_mounts() {
    let services = tls_stack_services();

    let trust = services
        .get("broker")
        .and_then(|broker| broker.get("environment"))
        .and_then(|environment| environment.get("ANDROMEDA_HTTP_FORWARDED_HEADERS"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    assert_eq!(
        trust.as_deref(),
        Some("trusted"),
        "the broker behind the terminator believes the fields it is sent"
    );

    let volumes = services
        .get("terminator")
        .and_then(|terminator| terminator.get("volumes"))
        .and_then(Value::as_sequence)
        .expect("the terminator mounts its configuration");
    assert!(
        volumes
            .iter()
            .filter_map(Value::as_str)
            .any(|mount| mount.starts_with("./nginx.conf:") && mount.ends_with(":ro")),
        "the terminator mounts {NGINX_CONFIGURATION} read-only: {volumes:?}"
    );
}
