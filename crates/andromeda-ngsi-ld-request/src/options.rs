//! Endpoint-specific interpretations of the HTTP `options` parameter.

use crate::{
    options_token::OptionsToken,
    representation_parameter::{RepresentationGrammar, RepresentationParameter, SelectedRepresentation},
    unsupported_representation::UnsupportedRepresentation,
};
use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use andromeda_ngsi_ld_query::{
    comma_separated_list::{ListedItem, listed_members},
    count_policy::CountPolicy,
};
use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use serde::Deserialize;
use strum::{Display, EnumString};

/// Entity list/retrieval options from clauses 6.3.7 and 6.3.11.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EntityRepresentationOptions {
    /// Deprecated representation selection, when supplied through `options`.
    pub format: Option<ResponseFormat>,
    /// Whether system-generated temporal attributes are included.
    pub sys_attrs: SysAttrsInclusion,
}

/// Temporal list/retrieval options from clauses 6.3.11 and 6.3.12.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TemporalRepresentationOptions {
    /// Deprecated temporal representation selection through `options`.
    pub format: Option<TemporalFormat>,
    /// Whether system-generated temporal attributes are included.
    pub sys_attrs: SysAttrsInclusion,
}

/// Subscription-list response options from clauses 6.3.13 and 6.3.11.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ListOptions {
    /// Whether the response emits the total result count.
    pub count: CountPolicy,
    /// Whether system-generated temporal attributes are included.
    pub sys_attrs: SysAttrsInclusion,
}

/// System-attribute selection for endpoints whose only `options` token is `sysAttrs`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SysAttrsOptions {
    /// Whether system-generated temporal attributes are included.
    pub sys_attrs: SysAttrsInclusion,
}

/// Whether an append/update operation may overwrite existing attributes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Display, EnumString)]
pub enum OverwritePolicy {
    /// Existing attributes may be overwritten.
    #[default]
    Overwrite,
    /// Existing attributes are preserved (`noOverwrite`).
    #[strum(serialize = "noOverwrite")]
    NoOverwrite,
}

/// Batch-upsert update mode from clause 6.15.3.1.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Display, EnumString, Deserialize)]
pub enum UpsertMode {
    /// Completely replace an entity that already exists.
    #[default]
    #[strum(serialize = "replace")]
    #[serde(rename = "replace")]
    Replace,
    /// Update the existing entity content.
    #[strum(serialize = "update")]
    #[serde(rename = "update")]
    Update,
}

impl SelectedRepresentation for EntityRepresentationOptions {
    type Representation = ResponseFormat;

    fn representation(&self) -> Option<ResponseFormat> {
        self.format
    }
}

impl SelectedRepresentation for TemporalRepresentationOptions {
    type Representation = TemporalFormat;

    fn representation(&self) -> Option<TemporalFormat> {
        self.format
    }
}

// Table 6.3.7-1 gives `options` the same three renderings as `format`, plus
// the `sysAttrs` token of clause 6.3.11. A repeated token and two tokens naming
// different renderings are both values the operation cannot act on, so clause
// 6.3.20 refuses them alongside a token it does not define.
impl RepresentationGrammar for EntityRepresentationOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<EntityRepresentationOptions, UnsupportedRepresentation> {
        let mut parsed = EntityRepresentationOptions::default();
        for token in option_tokens(written)? {
            match token {
                OptionsToken::Normalized => set_entity_format(&mut parsed, token, ResponseFormat::Normalized)?,
                OptionsToken::Concise => set_entity_format(&mut parsed, token, ResponseFormat::Concise)?,
                OptionsToken::Simplified | OptionsToken::KeyValues => set_entity_format(&mut parsed, token, ResponseFormat::Simplified)?,
                OptionsToken::SysAttrs => include_sys_attrs(&mut parsed.sys_attrs)?,
                OptionsToken::TemporalValues
                | OptionsToken::AggregatedValues
                | OptionsToken::GroupedValues
                | OptionsToken::Count
                | OptionsToken::NoOverwrite
                | OptionsToken::Replace
                | OptionsToken::Update => return Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
            }
        }
        Ok(parsed)
    }
}

// Table 6.3.12-1: "Only one of the two keywords can be present in the values of
// the parameter."
impl RepresentationGrammar for TemporalRepresentationOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<TemporalRepresentationOptions, UnsupportedRepresentation> {
        let mut parsed = TemporalRepresentationOptions::default();
        for token in option_tokens(written)? {
            match token {
                OptionsToken::TemporalValues => set_temporal_format(&mut parsed, token, TemporalFormat::TemporalValues)?,
                OptionsToken::AggregatedValues => set_temporal_format(&mut parsed, token, TemporalFormat::AggregatedValues)?,
                // Non-standard, from the temporal aggregation grouping
                // extension: the grouped counterpart of `aggregatedValues`,
                // named through `options` as well as `format`.
                OptionsToken::GroupedValues => set_temporal_format(&mut parsed, token, TemporalFormat::GroupedValues)?,
                OptionsToken::SysAttrs => include_sys_attrs(&mut parsed.sys_attrs)?,
                OptionsToken::Normalized
                | OptionsToken::Concise
                | OptionsToken::Simplified
                | OptionsToken::KeyValues
                | OptionsToken::Count
                | OptionsToken::NoOverwrite
                | OptionsToken::Replace
                | OptionsToken::Update => return Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
            }
        }
        Ok(parsed)
    }
}

// Clauses 6.3.13 and 6.3.11: a listing's `options` carries `count` and
// `sysAttrs` and nothing else.
impl RepresentationGrammar for ListOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<ListOptions, UnsupportedRepresentation> {
        let mut parsed = ListOptions::default();
        for token in option_tokens(written)? {
            match token {
                OptionsToken::Count => match parsed.count {
                    CountPolicy::Omit => parsed.count = CountPolicy::Emit,
                    CountPolicy::Emit => return Err(UnsupportedRepresentation::DuplicateOptionsToken(token)),
                },
                OptionsToken::SysAttrs => include_sys_attrs(&mut parsed.sys_attrs)?,
                OptionsToken::Normalized
                | OptionsToken::Concise
                | OptionsToken::Simplified
                | OptionsToken::KeyValues
                | OptionsToken::TemporalValues
                | OptionsToken::AggregatedValues
                | OptionsToken::GroupedValues
                | OptionsToken::NoOverwrite
                | OptionsToken::Replace
                | OptionsToken::Update => return Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
            }
        }
        Ok(parsed)
    }
}

// Clause 6.3.11 is the only `options` value an operation with no representation
// choice of its own accepts.
impl RepresentationGrammar for SysAttrsOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<SysAttrsOptions, UnsupportedRepresentation> {
        match single_token(written)? {
            OptionsToken::SysAttrs => Ok(SysAttrsOptions {
                sys_attrs: SysAttrsInclusion::Include,
            }),
            token @ (OptionsToken::Normalized
            | OptionsToken::Concise
            | OptionsToken::Simplified
            | OptionsToken::KeyValues
            | OptionsToken::TemporalValues
            | OptionsToken::AggregatedValues
            | OptionsToken::GroupedValues
            | OptionsToken::Count
            | OptionsToken::NoOverwrite
            | OptionsToken::Replace
            | OptionsToken::Update) => Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
        }
    }
}

// Clause 6.3.9: `noOverwrite` is the one value append and batch update define.
impl RepresentationGrammar for OverwritePolicy {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<OverwritePolicy, UnsupportedRepresentation> {
        match single_token(written)? {
            OptionsToken::NoOverwrite => Ok(OverwritePolicy::NoOverwrite),
            token @ (OptionsToken::Normalized
            | OptionsToken::Concise
            | OptionsToken::Simplified
            | OptionsToken::KeyValues
            | OptionsToken::SysAttrs
            | OptionsToken::TemporalValues
            | OptionsToken::AggregatedValues
            | OptionsToken::GroupedValues
            | OptionsToken::Count
            | OptionsToken::Replace
            | OptionsToken::Update) => Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
        }
    }
}

// Clause 6.15.3.1 defines the two upsert modes and no other `options` value.
impl RepresentationGrammar for UpsertMode {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<UpsertMode, UnsupportedRepresentation> {
        match single_token(written)? {
            OptionsToken::Replace => Ok(UpsertMode::Replace),
            OptionsToken::Update => Ok(UpsertMode::Update),
            token @ (OptionsToken::Normalized
            | OptionsToken::Concise
            | OptionsToken::Simplified
            | OptionsToken::KeyValues
            | OptionsToken::SysAttrs
            | OptionsToken::TemporalValues
            | OptionsToken::AggregatedValues
            | OptionsToken::GroupedValues
            | OptionsToken::Count
            | OptionsToken::NoOverwrite) => Err(UnsupportedRepresentation::UnsupportedOptionsToken(token)),
        }
    }
}

/// Read a value an operation admits exactly one token for.
fn single_token(written: &str) -> Result<OptionsToken, UnsupportedRepresentation> {
    written.parse().map_err(|_| UnsupportedRepresentation::UnknownOptionsToken)
}

/// Validate and read a comma-separated option list: an empty token anywhere
/// refuses the list before any token is read.
fn option_tokens(raw: &str) -> Result<Vec<OptionsToken>, UnsupportedRepresentation> {
    let tokens = listed_members(raw, ListedItem::OptionsToken)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| UnsupportedRepresentation::EmptyOptionsToken)?;
    tokens.into_iter().map(single_token).collect()
}

/// Ask for the system-generated temporal properties, refusing a second ask.
const fn include_sys_attrs(sys_attrs: &mut SysAttrsInclusion) -> Result<(), UnsupportedRepresentation> {
    match sys_attrs {
        SysAttrsInclusion::Strip => {
            *sys_attrs = SysAttrsInclusion::Include;
            Ok(())
        }
        SysAttrsInclusion::Include => Err(UnsupportedRepresentation::DuplicateOptionsToken(OptionsToken::SysAttrs)),
    }
}

/// Set an entity response format while rejecting conflicting tokens.
fn set_entity_format(options: &mut EntityRepresentationOptions, token: OptionsToken, format: ResponseFormat) -> Result<(), UnsupportedRepresentation> {
    match options.format {
        None => {
            options.format = Some(format);
            Ok(())
        }
        Some(existing) if existing == format => Err(UnsupportedRepresentation::DuplicateOptionsToken(token)),
        Some(_) => Err(UnsupportedRepresentation::ConflictingRepresentations),
    }
}

/// Set a temporal response format while rejecting conflicting tokens.
fn set_temporal_format(options: &mut TemporalRepresentationOptions, token: OptionsToken, format: TemporalFormat) -> Result<(), UnsupportedRepresentation> {
    match options.format {
        None => {
            options.format = Some(format);
            Ok(())
        }
        Some(existing) if existing == format => Err(UnsupportedRepresentation::DuplicateOptionsToken(token)),
        Some(_) => Err(UnsupportedRepresentation::ConflictingRepresentations),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        options::{EntityRepresentationOptions, ListOptions, OverwritePolicy, SysAttrsOptions, TemporalRepresentationOptions, UpsertMode},
        options_token::OptionsToken,
        representation_parameter::RepresentationGrammar,
        unsupported_representation::UnsupportedRepresentation,
    };
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_query::count_policy::CountPolicy;
    use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};

    #[test]
    fn entity_options_are_typed_and_conflicts_are_rejected() {
        let options = EntityRepresentationOptions::parse("keyValues,sysAttrs").expect("valid options");
        pretty_assertions::assert_eq!(options.format, Some(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(
            EntityRepresentationOptions::parse("normalized,concise"),
            Err(UnsupportedRepresentation::ConflictingRepresentations)
        );
        pretty_assertions::assert_eq!(
            EntityRepresentationOptions::parse("unknown"),
            Err(UnsupportedRepresentation::UnknownOptionsToken)
        );
        pretty_assertions::assert_eq!(
            EntityRepresentationOptions::parse("sysAttrs,sysAttrs"),
            Err(UnsupportedRepresentation::DuplicateOptionsToken(OptionsToken::SysAttrs))
        );
        pretty_assertions::assert_eq!(
            EntityRepresentationOptions::parse("keyValues,simplified"),
            Err(UnsupportedRepresentation::DuplicateOptionsToken(OptionsToken::Simplified))
        );
        pretty_assertions::assert_eq!(EntityRepresentationOptions::parse(""), Err(UnsupportedRepresentation::EmptyOptionsToken));
        pretty_assertions::assert_eq!(
            EntityRepresentationOptions::parse("count"),
            Err(UnsupportedRepresentation::UnsupportedOptionsToken(OptionsToken::Count))
        );
    }

    /// Clause 6.3.20: an unsupported token anywhere in the list is refused,
    /// not silently dropped, whichever position it takes.
    #[test]
    fn one_unsupported_token_refuses_the_whole_list() {
        assert!(EntityRepresentationOptions::parse("keyValues,invalid").is_err());
        assert!(EntityRepresentationOptions::parse("invalid,keyValues").is_err());
        assert!(TemporalRepresentationOptions::parse("temporalValues,invalidOptions").is_err());
    }

    #[test]
    fn temporal_options_are_typed_and_conflicts_are_rejected() {
        let options = TemporalRepresentationOptions::parse("temporalValues,sysAttrs").expect("valid options");
        pretty_assertions::assert_eq!(options.format, Some(TemporalFormat::TemporalValues));
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        assert!(TemporalRepresentationOptions::parse("temporalValues,aggregatedValues").is_err());
    }

    #[test]
    fn overwrite_options_accept_only_no_overwrite() {
        pretty_assertions::assert_eq!(OverwritePolicy::parse("noOverwrite"), Ok(OverwritePolicy::NoOverwrite));
        pretty_assertions::assert_eq!(OverwritePolicy::parse("overwrite"), Err(UnsupportedRepresentation::UnknownOptionsToken));
        pretty_assertions::assert_eq!(
            OverwritePolicy::parse("replace"),
            Err(UnsupportedRepresentation::UnsupportedOptionsToken(OptionsToken::Replace))
        );
    }

    #[test]
    fn upsert_options_accept_only_the_two_modes() {
        pretty_assertions::assert_eq!(UpsertMode::parse("replace"), Ok(UpsertMode::Replace));
        pretty_assertions::assert_eq!(UpsertMode::parse("update"), Ok(UpsertMode::Update));
        assert!(UpsertMode::parse("merge").is_err());
    }

    #[test]
    fn sys_attrs_options_accept_only_that_token() {
        pretty_assertions::assert_eq!(
            SysAttrsOptions::parse("sysAttrs"),
            Ok(SysAttrsOptions {
                sys_attrs: SysAttrsInclusion::Include
            })
        );
        assert!(SysAttrsOptions::parse("count").is_err());
    }

    #[test]
    fn list_options_are_typed_and_repeated_tokens_are_rejected() {
        let options = ListOptions::parse("count,sysAttrs").expect("valid options");
        pretty_assertions::assert_eq!(options.count, CountPolicy::Emit);
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        pretty_assertions::assert_eq!(
            ListOptions::parse("count,count"),
            Err(UnsupportedRepresentation::DuplicateOptionsToken(OptionsToken::Count))
        );
        assert!(ListOptions::parse("unknown").is_err());
        assert!(ListOptions::parse("").is_err());
    }
}
