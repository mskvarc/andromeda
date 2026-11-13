//! Every URI the conversion admits is one the broker reads. The conversion
//! tells a URI by patterns of its own, and a seal it kept with a URI the
//! broker refused would make its Entity unreadable, so the patterns are held
//! against the broker's parser over a corpus built around the points where
//! the two could part: the host, port, path and query of a URL, the opaque
//! path of every other scheme, and the characters of an IRI.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    statements::{name, texts},
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_postgresql_context_store::migration_set::migration_version::MigrationVersion;
use andromeda_postgresql_seaquery::{seal_conversion::seal_conversion_admits_uri, statement::Statement, unnest::unnest_of};
use sea_query::{Expr, Query};
use sqlx::PgPool;
use std::collections::HashSet;

/// The tenant migration that creates the conversion functions. The upgrade
/// drops them again, so the corpus is judged with the tenant stopped here.
const CONVERSION: MigrationVersion = MigrationVersion::new(32);

/// How many candidates one statement binds, below the protocol's bound on
/// the parameters of a statement.
const BATCH: usize = 5000;

/// Hosts of an `http` or `https` URL, valid and not.
const HOSTS: [&str; 18] = [
    "example.org",
    "localhost",
    "a.b.c",
    "a-b.example",
    "EXAMPLE.org",
    "xn--bcher-kva.example",
    "1.2.3.4",
    "example.1",
    "0x7f.1",
    "a_b.org",
    "-a.org",
    "a-.org",
    "[::1]",
    "user@example.org",
    "example.org.",
    "exa%41mple.org",
    "\u{e9}.org",
    "",
];

/// Ports, valid and not.
const PORTS: [&str; 9] = ["", ":80", ":443", ":0", ":00080", ":65535", ":65536", ":", ":8a"];

/// Paths, valid and not.
const PATHS: [&str; 14] = [
    "",
    "/",
    "/a",
    "/a/../b",
    "/./a",
    "/%2e%2e/a",
    "/a%20b",
    "/a%2",
    "/a b",
    "/\u{e9}",
    "/a'b",
    "//a",
    "/a;b=c",
    "/a\\b",
];

/// Queries and fragments, valid and not.
const TAILS: [&str; 10] = ["", "?", "?a=b", "?a'b", "?a b", "?%zz", "#", "#f", "#a#b", "?q#f"];

/// Schemes read with an opaque path, the special schemes of the WHATWG URL
/// Standard among them.
const SCHEMES: [&str; 14] = [
    "urn", "did", "mailto", "tag", "x", "a+b.c-d", "ftp", "file", "ws", "wss", "http", "https", "URN", "1x",
];

/// What follows the scheme and its colon, valid and not.
const OPAQUE: [&str; 36] = [
    "",
    "a",
    "a:b",
    "/a",
    "//a",
    "//a/b",
    "?a",
    "#a",
    "a?b#c",
    "a#b#c",
    "a b",
    " a",
    "a\tb",
    "a\nb",
    "a%zz",
    "a%41",
    "a/b/../c",
    "a'b",
    "a\\b",
    "a|b",
    "a[b]",
    "a{b}",
    "a\"b",
    "a<b>",
    "a^b",
    "a`b",
    "ngsi-ld:Store:002",
    "ngsi-ld:",
    "ngsi-ld:null",
    "x:y?+r?=q#f",
    "key:z6Mk#z6Mk",
    "\u{e9}",
    "a\u{a0}\u{d7ff}\u{f900}\u{fdcf}\u{fdf0}\u{ffef}",
    "a\u{fdd0}",
    "a\u{e000}",
    "a\u{10000}\u{efffd}",
];

/// URIs a seal ordinarily names, which the conversion must admit for the
/// comparison to mean anything.
const ORDINARY: [&str; 6] = [
    "urn:ngsi-ld:Store:002",
    "https://example.org/keys/1#key-1",
    "https://example.org:65535/a?a=b",
    "https://w3id.org/security#assertionMethod",
    "did:key:z6Mk#z6Mk",
    "https://uri.etsi.org/ngsi-ld/default-context/Store",
];

/// Every candidate: each URL part crossed with every other, each scheme with
/// every opaque path, and the ordinary URIs.
fn corpus() -> Vec<String> {
    let urls = ["http", "https"].into_iter().flat_map(|scheme| {
        HOSTS.into_iter().flat_map(move |host| {
            PORTS.into_iter().flat_map(move |port| {
                PATHS
                    .into_iter()
                    .flat_map(move |path| TAILS.into_iter().map(move |tail| format!("{scheme}://{host}{port}{path}{tail}")))
            })
        })
    });
    let opaque = SCHEMES
        .into_iter()
        .flat_map(|scheme| OPAQUE.into_iter().map(move |rest| format!("{scheme}:{rest}")));
    urls.chain(opaque).chain(ORDINARY.into_iter().map(str::to_owned)).collect()
}

/// The candidates of `batch` the conversion admits.
async fn admitted(pool: &PgPool, batch: &[String]) -> Result<Vec<String>, UpgradeTestError> {
    let candidate = name("candidate");
    let candidates: Vec<Option<&str>> = batch.iter().map(|uri| Some(uri.as_str())).collect();
    let select = Query::select()
        .column(candidate.clone())
        .from_function(unnest_of(texts(&candidates)), candidate.clone())
        .and_where(seal_conversion_admits_uri(Expr::col(candidate)))
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_all(pool).await?)
}

/// The broker stores the spelling its parser writes, which is at most the
/// slash of an empty path longer than the one written. The conversion bounds
/// an identifier one byte below the identifier bound on that account.
#[tokio::test]
async fn every_uri_the_conversion_admits_is_one_the_broker_reads() -> Result<(), UpgradeTestError> {
    let (database, _) = at_v1_0_0().await?;
    database.migrate_public_through(CONVERSION).await?;
    let corpus = corpus();
    let mut kept = HashSet::new();
    for batch in corpus.chunks(BATCH) {
        kept.extend(admitted(database.pool(), batch).await?);
    }

    for uri in &kept {
        let Ok(read) = uri.parse::<Uri>() else {
            panic!("the conversion admits {uri:?}, which the broker refuses");
        };
        let stored = read.as_str().len();
        assert!(stored <= uri.len() + 1, "the broker stores {uri:?} in {stored} bytes");
        if uri.len() < 1024 {
            assert!(uri.parse::<Identifier>().is_ok(), "the conversion admits {uri:?}, which is no identifier");
        }
    }
    for uri in ORDINARY {
        assert!(kept.contains(uri), "the conversion refuses {uri:?}");
    }
    assert!(kept.len() < corpus.len(), "the corpus holds URIs the conversion refuses");
    for refused in [
        "https://example.org:65536/a",
        "https://[::1]/a",
        "file:a",
        "x:a b",
        "x:a#b#c",
        "x:a\u{fdd0}",
        "URN:a",
    ] {
        assert!(corpus.iter().any(|uri| uri == refused), "{refused:?} is in the corpus");
        assert!(!kept.contains(refused), "the conversion admits {refused:?}");
    }
    database.drop_database().await?;
    Ok(())
}
