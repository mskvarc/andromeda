//! The NGSI-LD version profile a `Link` header can carry (clause 6.3.21).
//!
//! Clause 6.3.21 lets a request announce, through an IETF RFC 6906 profile,
//! which NGSI-LD version its payload is written against:
//! `<https://uri.etsi.org/ngsi-ld/profile/1.8>`. RFC 6906 registers `profile`
//! as a link relation and RFC 8288 section 3.4 also defines a `profile` target
//! attribute, so the same announcement is read from either spelling:
//!
//! ```text
//! Link: <https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile"
//! Link: <https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; profile="https://uri.etsi.org/ngsi-ld/profile/1.8"
//! ```

use crate::link::{header::Link, relation_type::RelationType, value::LinkValue};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use iri_rs::{Iri, iri};
use std::str::FromStr;

/// The prefix every NGSI-LD version profile IRI carries (clause 6.3.21); the
/// remainder is the version in `major.minor` form.
const NGSI_LD_PROFILE_PREFIX: Iri<&'static str> = iri!("https://uri.etsi.org/ngsi-ld/profile/");

/// The version an NGSI-LD profile IRI names, if `candidate` is one.
fn profile_version(candidate: &str) -> Option<SpecVersion> {
    candidate
        .strip_prefix(NGSI_LD_PROFILE_PREFIX.as_str())
        .and_then(|version| SpecVersion::from_str(version).ok())
}

impl LinkValue {
    /// The NGSI-LD version this link-value announces as its profile (clause
    /// 6.3.21), from either its `profile` attribute or, when it carries
    /// `rel="profile"`, its target.
    ///
    /// A profile IRI outside the NGSI-LD version namespace, or one whose
    /// remainder is not a `major.minor` version, announces nothing.
    #[must_use]
    pub fn ngsi_ld_profile_version(&self) -> Option<SpecVersion> {
        let attribute = self.profile().and_then(|profile| profile_version(profile.as_str()));
        attribute.or_else(|| self.has_rel(&RelationType::Profile).then(|| profile_version(self.link().as_str())).flatten())
    }
}

impl Link {
    /// The NGSI-LD version the first profile-bearing link-value announces
    /// (clause 6.3.21), if any does.
    pub fn ngsi_ld_profile_version(&self) -> Option<SpecVersion> {
        self.values().iter().find_map(LinkValue::ngsi_ld_profile_version)
    }
}

#[cfg(test)]
mod tests {
    use crate::link::{header::Link, relation_type::RelationType, value::LinkValue};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use iri_rs::IriRefBuf;
    use std::str::FromStr;

    /// A validated IRI reference, for building link targets in tests.
    fn iri_ref(value: &str) -> IriRefBuf {
        IriRefBuf::new(value).expect("valid IRI reference")
    }

    #[test]
    fn a_profile_relation_naming_an_ngsi_ld_version_is_read_from_the_target() {
        let link = Link::from_str(r#"<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile""#).expect("a profile link parses");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), Some(SpecVersion::V1_8));
    }

    #[test]
    fn a_profile_attribute_on_a_context_link_is_read_alongside_the_context() {
        let raw = r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; profile="https://uri.etsi.org/ngsi-ld/profile/1.4""#;
        let link = Link::from_str(raw).expect("a context link with a profile parses");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), Some(SpecVersion::V1_4));
        pretty_assertions::assert_eq!(link.context_urls().count(), 1);
    }

    #[test]
    fn the_profile_attribute_wins_over_a_profile_relation_target() {
        let mut value = LinkValue::new(iri_ref("https://uri.etsi.org/ngsi-ld/profile/1.3")).push_rel(RelationType::Profile);
        value
            .apply_param(r#"profile="https://uri.etsi.org/ngsi-ld/profile/1.4""#)
            .expect("a profile parameter applies");
        pretty_assertions::assert_eq!(value.ngsi_ld_profile_version(), Some(SpecVersion::V1_4));
    }

    #[test]
    fn a_profile_outside_the_ngsi_ld_namespace_announces_no_version() {
        let link = Link::from_str(r#"<https://example.com/profiles/1.8>; rel="profile""#).expect("a foreign profile link parses");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), None);
    }

    #[test]
    fn a_profile_whose_remainder_is_not_a_version_announces_no_version() {
        let link = Link::from_str(r#"<https://uri.etsi.org/ngsi-ld/profile/latest>; rel="profile""#).expect("the link parses");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), None);
    }

    #[test]
    fn a_version_target_without_the_profile_relation_announces_no_version() {
        let link = Link::from_str(r#"<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="next""#).expect("the link parses");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), None);
    }

    #[test]
    fn the_first_profile_bearing_value_decides_among_several() {
        let raw = r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context", <https://uri.etsi.org/ngsi-ld/profile/1.8>; rel="profile""#;
        let link = Link::from_str(raw).expect("two link-values parse");
        pretty_assertions::assert_eq!(link.ngsi_ld_profile_version(), Some(SpecVersion::V1_8));
    }
}
