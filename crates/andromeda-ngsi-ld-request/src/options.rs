//! Endpoint-specific interpretations of the HTTP `options` parameter.

use crate::representation_parameter::{RepresentationGrammar, RepresentationParameter, SelectedRepresentation};
use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use andromeda_ngsi_ld_query::pagination::CountPolicy;
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

    fn parse(written: &str) -> Result<EntityRepresentationOptions, String> {
        let mut parsed = EntityRepresentationOptions::default();
        for token in option_tokens(written)? {
            match token {
                "normalized" => set_entity_format(&mut parsed, ResponseFormat::Normalized)?,
                "concise" => set_entity_format(&mut parsed, ResponseFormat::Concise)?,
                "simplified" | "keyValues" => set_entity_format(&mut parsed, ResponseFormat::Simplified)?,
                "sysAttrs" if parsed.sys_attrs == SysAttrsInclusion::Strip => parsed.sys_attrs = SysAttrsInclusion::Include,
                "sysAttrs" => return Err("duplicate options token 'sysAttrs'".to_owned()),
                other => return Err(format!("unsupported options token '{other}'")),
            }
        }
        Ok(parsed)
    }
}

// Table 6.3.12-1: "Only one of the two keywords can be present in the values of
// the parameter."
impl RepresentationGrammar for TemporalRepresentationOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<TemporalRepresentationOptions, String> {
        let mut parsed = TemporalRepresentationOptions::default();
        for token in option_tokens(written)? {
            match token {
                "temporalValues" => set_temporal_format(&mut parsed, TemporalFormat::TemporalValues)?,
                "aggregatedValues" => set_temporal_format(&mut parsed, TemporalFormat::AggregatedValues)?,
                // Non-standard, from the temporal aggregation grouping
                // extension: the grouped counterpart of `aggregatedValues`,
                // named through `options` as well as `format`.
                "groupedValues" => set_temporal_format(&mut parsed, TemporalFormat::GroupedValues)?,
                "sysAttrs" if parsed.sys_attrs == SysAttrsInclusion::Strip => parsed.sys_attrs = SysAttrsInclusion::Include,
                "sysAttrs" => return Err("duplicate options token 'sysAttrs'".to_owned()),
                other => return Err(format!("unsupported options token '{other}'")),
            }
        }
        Ok(parsed)
    }
}

// Clauses 6.3.13 and 6.3.11: a listing's `options` carries `count` and
// `sysAttrs` and nothing else.
impl RepresentationGrammar for ListOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<ListOptions, String> {
        let mut parsed = ListOptions::default();
        for token in option_tokens(written)? {
            match token {
                "count" if parsed.count == CountPolicy::Omit => parsed.count = CountPolicy::Emit,
                "count" => return Err("duplicate options token 'count'".to_owned()),
                "sysAttrs" if parsed.sys_attrs == SysAttrsInclusion::Strip => parsed.sys_attrs = SysAttrsInclusion::Include,
                "sysAttrs" => return Err("duplicate options token 'sysAttrs'".to_owned()),
                other => return Err(format!("unsupported options token '{other}'")),
            }
        }
        Ok(parsed)
    }
}

// Clause 6.3.11 is the only `options` value an operation with no representation
// choice of its own accepts.
impl RepresentationGrammar for SysAttrsOptions {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<SysAttrsOptions, String> {
        match written {
            "sysAttrs" => Ok(SysAttrsOptions {
                sys_attrs: SysAttrsInclusion::Include,
            }),
            other => Err(format!("unsupported options token '{other}'")),
        }
    }
}

// Clause 6.3.9: `noOverwrite` is the one value append and batch update define.
impl RepresentationGrammar for OverwritePolicy {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<OverwritePolicy, String> {
        match written {
            "noOverwrite" => Ok(OverwritePolicy::NoOverwrite),
            other => Err(format!("unsupported options token '{other}'")),
        }
    }
}

// Clause 6.15.3.1 defines the two upsert modes and no other `options` value.
impl RepresentationGrammar for UpsertMode {
    const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

    fn parse(written: &str) -> Result<UpsertMode, String> {
        match written {
            "replace" => Ok(UpsertMode::Replace),
            "update" => Ok(UpsertMode::Update),
            other => Err(format!("unsupported options token '{other}'")),
        }
    }
}

/// Validate and iterate a comma-separated option list.
fn option_tokens(raw: &str) -> Result<impl Iterator<Item = &str>, String> {
    if raw.is_empty() || raw.split(',').any(|token| token.trim().is_empty()) {
        return Err("options must contain non-empty comma-separated tokens".to_owned());
    }
    Ok(raw.split(',').map(str::trim))
}

/// Set an entity response format while rejecting conflicting tokens.
fn set_entity_format(options: &mut EntityRepresentationOptions, format: ResponseFormat) -> Result<(), String> {
    match options.format {
        None => {
            options.format = Some(format);
            Ok(())
        }
        Some(existing) if existing == format => Err(format!("duplicate representation option '{format}'")),
        Some(_) => Err("conflicting representation options".to_owned()),
    }
}

/// Set a temporal response format while rejecting conflicting tokens.
fn set_temporal_format(options: &mut TemporalRepresentationOptions, format: TemporalFormat) -> Result<(), String> {
    match options.format {
        None => {
            options.format = Some(format);
            Ok(())
        }
        Some(existing) if existing == format => Err(format!("duplicate temporal representation option '{format}'")),
        Some(_) => Err("conflicting temporal representation options".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        options::{EntityRepresentationOptions, ListOptions, OverwritePolicy, SysAttrsOptions, TemporalRepresentationOptions, UpsertMode},
        representation_parameter::RepresentationGrammar,
    };
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_query::pagination::CountPolicy;
    use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};

    #[test]
    fn entity_options_are_typed_and_conflicts_are_rejected() {
        let options = EntityRepresentationOptions::parse("keyValues,sysAttrs").expect("valid options");
        pretty_assertions::assert_eq!(options.format, Some(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        assert!(EntityRepresentationOptions::parse("normalized,concise").is_err());
        assert!(EntityRepresentationOptions::parse("unknown").is_err());
        assert!(EntityRepresentationOptions::parse("sysAttrs,sysAttrs").is_err());
        assert!(EntityRepresentationOptions::parse("").is_err());
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
        assert!(OverwritePolicy::parse("overwrite").is_err());
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
        assert!(ListOptions::parse("count,count").is_err());
        assert!(ListOptions::parse("unknown").is_err());
        assert!(ListOptions::parse("").is_err());
    }
}
