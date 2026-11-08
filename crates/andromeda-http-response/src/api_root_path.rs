//! The path every path the broker hands out starts with.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.2: "The apiRoot includes the scheme
//! ("http" or "https"), host and optional port, and an optional prefix
//! string." The broker serves its routes only under `/ngsi-ld/v1`, so a
//! reverse proxy serving it under a prefix strips the prefix before the
//! request reaches the broker, and the request's own path never carries it.
//! Every path the broker hands out, a `Location` or a `Link` target, is
//! therefore built on the configured `apiRoot`'s path, so a client resolving
//! it against the URI it addressed reaches the broker through the same prefix.

use url::Url;

/// The path `api_root` puts before every path the broker serves: its own path
/// without a trailing slash, empty for an `apiRoot` without a path, and empty
/// when none is configured.
#[must_use]
pub fn api_root_path(api_root: Option<&Url>) -> &str {
    api_root.map_or("", |root| root.path().trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use crate::api_root_path::api_root_path;
    use url::Url;

    fn root(raw: &str) -> Url {
        Url::parse(raw).expect("a valid URL")
    }

    #[test]
    fn a_prefixed_api_root_puts_its_path_before_every_path() {
        let api_root = root("https://proxy.example/context-broker");
        pretty_assertions::assert_eq!(api_root_path(Some(&api_root)), "/context-broker");
    }

    #[test]
    fn a_trailing_slash_of_the_api_root_is_dropped() {
        let api_root = root("https://proxy.example/context-broker/");
        pretty_assertions::assert_eq!(api_root_path(Some(&api_root)), "/context-broker");
    }

    #[test]
    fn an_api_root_without_a_path_puts_nothing_before_a_path() {
        let api_root = root("https://proxy.example");
        pretty_assertions::assert_eq!(api_root_path(Some(&api_root)), "");
    }

    #[test]
    fn without_a_configured_api_root_nothing_goes_before_a_path() {
        pretty_assertions::assert_eq!(api_root_path(None), "");
    }
}
