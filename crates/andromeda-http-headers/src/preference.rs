//! The token vocabulary shared by the `Prefer` and `Preference-Applied` headers.
//!
//! RFC 7240 gives both headers the same grammar -- a comma-separated list of
//! tokens, each optionally `name=value` -- and NGSI-LD clause 6.3.6 fixes which
//! tokens a broker understands. The values those tokens carry are
//! representation options and live in `andromeda-ngsi-ld-representation`; what
//! lives here is how they are spelled on the wire.

use crate::geo_json_body::GeoJsonBodyPreference;
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use std::{fmt, str::FromStr};
use strum::{Display, EnumString};

/// Whether the client asked the server to process the request asynchronously
/// (RFC 7240 section 4.1).
///
/// The rendering is the whole token, because unlike `body` and `ngsi-ld` this
/// preference carries no value: `respond-async` and `respond-sync` are two
/// tokens rather than two values of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum RespondMode {
    /// `respond-async` -- the server may process the request asynchronously.
    #[strum(serialize = "respond-async")]
    Async,
    /// `respond-sync` -- the server must process the request synchronously,
    /// which is a stronger statement than the absence of the token.
    #[strum(serialize = "respond-sync")]
    Sync,
}

/// One NGSI-LD preference token (clause 6.3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    /// `body=json` or `body=ld+json` -- GeoJSON `@context` placement.
    Body(GeoJsonBodyPreference),
    /// `ngsi-ld=<version>` -- request version conformance (clause 4.3.6.8).
    Version(SpecVersion),
    /// `respond-async` / `respond-sync` -- response-mode hint. The
    /// `/entityOperations` endpoints read it to route side effects between
    /// inline processing and the durable queue.
    Respond(RespondMode),
}

impl Preference {
    /// Parse one trimmed token from a header value.
    ///
    /// Returns `None` for anything unrecognised, including a `ngsi-ld=` version
    /// this build does not implement: RFC 7240 makes preferences advisory, so a
    /// server ignores what it cannot honour rather than rejecting the request.
    pub fn parse(token: &str) -> Option<Preference> {
        if let Ok(mode) = RespondMode::from_str(token) {
            return Some(Preference::Respond(mode));
        }

        let (name, value) = token.split_once('=')?;
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("body") {
            GeoJsonBodyPreference::from_str(value).ok().map(Preference::Body)
        } else if name.eq_ignore_ascii_case("ngsi-ld") {
            SpecVersion::from_str(value).ok().map(Preference::Version)
        } else {
            None
        }
    }
}

// Each variant spells its name differently -- two carry a value after `=` and
// one is a bare token -- so no single strum serialisation covers the enum. The
// values themselves derive their own renderings.
impl fmt::Display for Preference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Preference::Body(body) => write!(f, "body={body}"),
            Preference::Version(version) => write!(f, "ngsi-ld={version}"),
            Preference::Respond(mode) => fmt::Display::fmt(mode, f),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        geo_json_body::GeoJsonBodyPreference,
        preference::{Preference, RespondMode},
    };
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;

    #[test]
    fn every_token_parses_to_its_preference() {
        pretty_assertions::assert_eq!(Preference::parse("body=json"), Some(Preference::Body(GeoJsonBodyPreference::Json)));
        pretty_assertions::assert_eq!(Preference::parse("body=ld+json"), Some(Preference::Body(GeoJsonBodyPreference::LdJson)));
        pretty_assertions::assert_eq!(Preference::parse("ngsi-ld=1.4"), Some(Preference::Version(SpecVersion::V1_4)));
        pretty_assertions::assert_eq!(Preference::parse("respond-async"), Some(Preference::Respond(RespondMode::Async)));
        pretty_assertions::assert_eq!(Preference::parse("respond-sync"), Some(Preference::Respond(RespondMode::Sync)));
    }

    #[test]
    fn every_preference_renders_and_parses_back() {
        for preference in [
            Preference::Body(GeoJsonBodyPreference::Json),
            Preference::Body(GeoJsonBodyPreference::LdJson),
            Preference::Version(SpecVersion::V1_9),
            Preference::Respond(RespondMode::Async),
            Preference::Respond(RespondMode::Sync),
        ] {
            pretty_assertions::assert_eq!(Preference::parse(&preference.to_string()), Some(preference));
        }
    }

    #[test]
    fn a_token_parses_regardless_of_case() {
        pretty_assertions::assert_eq!(Preference::parse("BODY=JSON"), Some(Preference::Body(GeoJsonBodyPreference::Json)));
        pretty_assertions::assert_eq!(Preference::parse("Respond-Async"), Some(Preference::Respond(RespondMode::Async)));
    }

    #[test]
    fn an_unhonourable_preference_is_ignored_rather_than_rejected() {
        pretty_assertions::assert_eq!(Preference::parse("wait=5"), None);
        pretty_assertions::assert_eq!(Preference::parse("return=minimal"), None);
        pretty_assertions::assert_eq!(Preference::parse("ngsi-ld=latest"), None);
        pretty_assertions::assert_eq!(Preference::parse("body=xml"), None);
    }
}
