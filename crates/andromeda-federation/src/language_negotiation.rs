//! What a read's `q` target languages may be, and how they are matched,
//! across the sources it reaches.
//!
//! The `q_target_languages` extension widens what a bracket may name and, at
//! its `language-ranges` level, changes what `[en]` selects. A source serving
//! a lower level refuses a target language it does not admit, and a
//! distributed read whose sources disagree on the matching would merge answers
//! to two different questions. A read whose `q` names a target language
//! therefore asks every source it reaches which level it advertises (clause
//! 5.2.40 `contextSourceExtras`). A read naming a target language some source
//! does not admit is refused. Otherwise the whole read runs at the least level
//! every source serves, locally and remotely, and a read that falls back from
//! extended filtering to the clause 4.9 tag equality says so with an
//! `NGSILD-Warning` (clause 6.3.17). How a source spells the advertisement is
//! the extension set's to say, so the caller hands in the reader.

use crate::{
    FederationService,
    error::FederationError,
    participating_read::ParticipatingRead,
    source_identity::{ReachedSource, SourceConfiguration},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::filter::{expression::Expr, language_matching::LanguageMatching, language_targeting::LanguageTargeting};
use thiserror::Error;

/// The level a read runs under, and the warning its fallback carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedLanguageTargeting {
    /// What the read's target languages may be and how they select
    /// languageMap keys, locally and at every source it reaches.
    pub targeting: LanguageTargeting,
    /// The clause 6.3.17 warning a fallback to tag equality carries, `None`
    /// when the read matches as the broker's own level does.
    pub warning: Option<NgsiLdWarning>,
}

impl NegotiatedLanguageTargeting {
    /// The broker's own level, with nothing to warn about.
    const fn unchanged(targeting: LanguageTargeting) -> NegotiatedLanguageTargeting {
        NegotiatedLanguageTargeting { targeting, warning: None }
    }
}

impl FederationService {
    /// The level `read` runs under when the broker's own is `broker`,
    /// `advertised` reading the level a source's `contextSourceExtras`
    /// advertise.
    ///
    /// Only a read whose `q` names a target language beyond the clause 4.9
    /// grammar, or a broker filtering by range, asks the sources it reaches
    /// anything: below both, every source reads the target languages alike.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::TargetLanguageUnadvertised`] when the `q`
    /// names a target language a reached source does not advertise the level
    /// of, and the failures of reading the participating registrations
    /// otherwise.
    pub async fn negotiate_language_targeting(
        &self,
        target: &ContextTarget,
        context: &ContextSpec,
        read: &ParticipatingRead<'_>,
        broker: LanguageTargeting,
        advertised: impl Fn(Option<&JsonLiteral>) -> LanguageTargeting,
    ) -> Result<NegotiatedLanguageTargeting, FederationError> {
        let Some(q) = read.client_query.q().filter(|q| names_target_language(q)) else {
            return Ok(NegotiatedLanguageTargeting::unchanged(broker));
        };
        let required = LanguageTargeting::required_by(q);
        if required == LanguageTargeting::Specification && broker.matching() == LanguageMatching::TagEquality {
            return Ok(NegotiatedLanguageTargeting::unchanged(broker));
        }
        negotiated(broker, required, &self.query_configurations(target, context, read).await?, advertised)
    }
}

/// Whether `q` names a target language at all.
fn names_target_language(q: &Expr) -> bool {
    !q.target_languages().is_empty()
}

/// The level a read needing `required` runs under across `reached` when the
/// broker's own is `broker`.
fn negotiated(
    broker: LanguageTargeting,
    required: LanguageTargeting,
    reached: &[ReachedSource],
    advertised: impl Fn(Option<&JsonLiteral>) -> LanguageTargeting,
) -> Result<NegotiatedLanguageTargeting, FederationError> {
    let served = |source: &ReachedSource| match &source.configuration {
        SourceConfiguration::Described(extras) => advertised(extras.as_ref()),
        SourceConfiguration::Undescribed => LanguageTargeting::Specification,
    };
    if let Some(short) = reached.iter().find(|source| served(source) < required) {
        return Err(FederationError::TargetLanguageUnadvertised {
            // clone: the error names the registration it outlives
            registration: short.registration.clone(),
            needs: required,
        });
    }
    let Some((weakest, level)) = reached
        .iter()
        .map(|source| (source, served(source)))
        .filter(|(_, level)| *level < broker)
        .min_by_key(|(_, level)| *level)
    else {
        return Ok(NegotiatedLanguageTargeting::unchanged(broker));
    };
    Ok(NegotiatedLanguageTargeting {
        targeting: level,
        warning: (level.matching() != broker.matching()).then(|| fallback_warning(&weakest.registration)),
    })
}

/// A read evaluated by tag equality because a source it reaches does not
/// advertise extended filtering.
#[derive(Debug, Error)]
#[error("language range filtering not applied: registration {registration} does not advertise languageRangeFiltering")]
struct RangeFilteringNotApplied<'a> {
    /// The registration naming the source.
    registration: &'a Identifier,
}

/// The clause 6.3.17 warning a read evaluated by tag equality carries.
fn fallback_warning(registration: &Identifier) -> NgsiLdWarning {
    NgsiLdWarning::new(WarnCode::MiscellaneousWarning, RangeFilteringNotApplied { registration })
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        language_negotiation::{NegotiatedLanguageTargeting, negotiated},
        source_identity::{ReachedSource, SourceConfiguration},
    };
    use andromeda_http_headers::ngsild_warning::WarnCode;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
    use pretty_assertions::assert_eq;

    /// The advertisement as the extension set spells it.
    fn advertised(extras: Option<&JsonLiteral>) -> LanguageTargeting {
        match extras.map(JsonLiteral::text) {
            Some(text) if text.contains("\"languageRangeFiltering\"") => LanguageTargeting::LanguageRanges,
            Some(text) if text.contains("\"languageTags\"") => LanguageTargeting::LanguageTags,
            Some(_) | None => LanguageTargeting::Specification,
        }
    }

    fn source(registration: &str, configuration: SourceConfiguration) -> ReachedSource {
        ReachedSource {
            registration: registration.parse().expect("a registration identifier"),
            configuration,
        }
    }

    fn advertising(registration: &str, tokens: &str) -> ReachedSource {
        source(
            registration,
            SourceConfiguration::Described(Some(JsonLiteral::admit(&format!(r#"{{"extensions": [{tokens}]}}"#)).expect("extras"))),
        )
    }

    fn ranges(registration: &str) -> ReachedSource {
        advertising(registration, r#""languageTags", "languageRangeFiltering""#)
    }

    fn tags(registration: &str) -> ReachedSource {
        advertising(registration, r#""languageTags""#)
    }

    #[test]
    fn every_source_serving_the_broker_level_keeps_it() {
        let outcome = negotiated(
            LanguageTargeting::LanguageRanges,
            LanguageTargeting::Specification,
            &[ranges("urn:ngsi-ld:ContextSourceRegistration:a")],
            advertised,
        )
        .expect("negotiated");
        assert_eq!(outcome, NegotiatedLanguageTargeting::unchanged(LanguageTargeting::LanguageRanges));
        let alone = negotiated(LanguageTargeting::LanguageRanges, LanguageTargeting::Specification, &[], advertised).expect("negotiated");
        assert_eq!(alone.targeting, LanguageTargeting::LanguageRanges);
    }

    #[test]
    fn one_undescribed_source_falls_back_to_tag_equality_with_a_warning() {
        let reached = [
            ranges("urn:ngsi-ld:ContextSourceRegistration:a"),
            source("urn:ngsi-ld:ContextSourceRegistration:b", SourceConfiguration::Undescribed),
        ];
        let outcome = negotiated(LanguageTargeting::LanguageRanges, LanguageTargeting::Specification, &reached, advertised).expect("negotiated");
        assert_eq!(outcome.targeting, LanguageTargeting::Specification);
        assert!(
            matches!(&outcome.warning, Some(warning) if warning.code() == WarnCode::MiscellaneousWarning && warning.text().as_str().contains("urn:ngsi-ld:ContextSourceRegistration:b")),
            "{outcome:?}"
        );
    }

    /// A source serving full tags but not ranges still matches by tag
    /// equality, so a filtering broker reading `[en-GB]` through it falls back.
    #[test]
    fn a_source_serving_tags_alone_takes_a_full_tag_by_tag_equality() {
        let outcome = negotiated(
            LanguageTargeting::LanguageRanges,
            LanguageTargeting::LanguageTags,
            &[tags("urn:ngsi-ld:ContextSourceRegistration:c")],
            advertised,
        )
        .expect("negotiated");
        assert_eq!(outcome.targeting, LanguageTargeting::LanguageTags);
        assert!(outcome.warning.is_some(), "{outcome:?}");
    }

    /// Below extended filtering every level matches by tag equality, so
    /// running at a lower one changes nothing the client could see.
    #[test]
    fn a_tags_broker_reaching_a_specification_source_runs_without_a_warning() {
        let outcome = negotiated(
            LanguageTargeting::LanguageTags,
            LanguageTargeting::Specification,
            &[advertising("urn:ngsi-ld:ContextSourceRegistration:d", r#""cursorPagination""#)],
            advertised,
        )
        .expect("negotiated");
        assert_eq!(outcome, NegotiatedLanguageTargeting::unchanged(LanguageTargeting::Specification));
    }

    #[test]
    fn a_full_tag_reaching_a_source_without_language_tags_is_refused() {
        let error = negotiated(
            LanguageTargeting::LanguageTags,
            LanguageTargeting::LanguageTags,
            &[
                tags("urn:ngsi-ld:ContextSourceRegistration:a"),
                source("urn:ngsi-ld:ContextSourceRegistration:b", SourceConfiguration::Undescribed),
            ],
            advertised,
        )
        .expect_err("the source refuses a full tag");
        assert!(
            matches!(&error, FederationError::TargetLanguageUnadvertised { registration, needs: LanguageTargeting::LanguageTags }
                if registration.as_str() == "urn:ngsi-ld:ContextSourceRegistration:b"),
            "{error:?}"
        );
    }

    #[test]
    fn a_range_reaching_a_source_without_language_ranges_is_refused() {
        let error = negotiated(
            LanguageTargeting::LanguageRanges,
            LanguageTargeting::LanguageRanges,
            &[tags("urn:ngsi-ld:ContextSourceRegistration:b")],
            advertised,
        )
        .expect_err("a range has no tag-equality reading");
        assert!(
            matches!(&error, FederationError::TargetLanguageUnadvertised { registration, needs: LanguageTargeting::LanguageRanges }
                if registration.as_str() == "urn:ngsi-ld:ContextSourceRegistration:b"),
            "{error:?}"
        );
    }
}
