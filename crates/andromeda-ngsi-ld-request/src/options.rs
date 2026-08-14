//! Endpoint-specific interpretations of the HTTP `options` parameter.

use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use andromeda_ngsi_ld_query::pagination::CountPolicy;
use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use serde::{Deserialize, Deserializer, de::Error};
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

/// Deserialize optional entity representation options, rejecting unknown,
/// repeated, and conflicting tokens.
pub fn deserialize_entity_representation_options<'de, D>(deserializer: D) -> Result<Option<EntityRepresentationOptions>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| parse_entity_options(&raw).map_err(Error::custom))
        .transpose()
}

/// Deserialize optional temporal representation options, rejecting unknown,
/// repeated, and conflicting tokens.
pub fn deserialize_temporal_representation_options<'de, D>(deserializer: D) -> Result<Option<TemporalRepresentationOptions>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| parse_temporal_options(&raw).map_err(Error::custom))
        .transpose()
}

/// Deserialize subscription-list options, rejecting unknown and repeated tokens.
pub fn deserialize_list_options<'de, D>(deserializer: D) -> Result<Option<ListOptions>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| parse_list_options(&raw).map_err(Error::custom))
        .transpose()
}

/// Deserialize the singleton `sysAttrs` option, rejecting every other token.
pub fn deserialize_sys_attrs_options<'de, D>(deserializer: D) -> Result<Option<SysAttrsOptions>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| match raw.as_str() {
            "sysAttrs" => Ok(SysAttrsOptions {
                sys_attrs: SysAttrsInclusion::Include,
            }),
            other => Err(Error::custom(format!("unsupported options value '{other}'"))),
        })
        .transpose()
}

/// Deserialize the singleton `noOverwrite` option used by append and batch update.
pub fn deserialize_overwrite_policy<'de, D>(deserializer: D) -> Result<Option<OverwritePolicy>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| match raw.as_str() {
            "noOverwrite" => Ok(OverwritePolicy::NoOverwrite),
            other => Err(Error::custom(format!("unsupported options value '{other}'"))),
        })
        .transpose()
}

/// Parse entity representation and system-attribute option tokens.
fn parse_entity_options(raw: &str) -> Result<EntityRepresentationOptions, String> {
    let mut parsed = EntityRepresentationOptions::default();
    for token in option_tokens(raw)? {
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

/// Parse temporal representation option tokens.
fn parse_temporal_options(raw: &str) -> Result<TemporalRepresentationOptions, String> {
    let mut parsed = TemporalRepresentationOptions::default();
    for token in option_tokens(raw)? {
        match token {
            "temporalValues" => set_temporal_format(&mut parsed, TemporalFormat::TemporalValues)?,
            "aggregatedValues" => set_temporal_format(&mut parsed, TemporalFormat::AggregatedValues)?,
            "sysAttrs" if parsed.sys_attrs == SysAttrsInclusion::Strip => parsed.sys_attrs = SysAttrsInclusion::Include,
            "sysAttrs" => return Err("duplicate options token 'sysAttrs'".to_owned()),
            other => return Err(format!("unsupported options token '{other}'")),
        }
    }
    Ok(parsed)
}

/// Parse subscription list-option tokens.
fn parse_list_options(raw: &str) -> Result<ListOptions, String> {
    let mut parsed = ListOptions::default();
    for token in option_tokens(raw)? {
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
    use crate::options::{EntityRepresentationOptions, ListOptions, OverwritePolicy, TemporalRepresentationOptions};
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_query::pagination::CountPolicy;
    use andromeda_ngsi_ld_representation::{response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct EntityWire {
        #[serde(default, deserialize_with = "crate::options::deserialize_entity_representation_options")]
        options: Option<EntityRepresentationOptions>,
    }

    #[derive(Deserialize)]
    struct TemporalWire {
        #[serde(default, deserialize_with = "crate::options::deserialize_temporal_representation_options")]
        options: Option<TemporalRepresentationOptions>,
    }

    #[derive(Deserialize)]
    struct OverwriteWire {
        #[serde(default, deserialize_with = "crate::options::deserialize_overwrite_policy")]
        options: Option<OverwritePolicy>,
    }

    #[derive(Deserialize)]
    struct ListWire {
        #[serde(default, deserialize_with = "crate::options::deserialize_list_options")]
        options: Option<ListOptions>,
    }

    #[test]
    fn entity_options_are_typed_and_conflicts_are_rejected() {
        let parsed: EntityWire = serde_urlencoded::from_str("options=keyValues%2CsysAttrs").expect("valid options");
        let options = parsed.options.expect("options present");
        pretty_assertions::assert_eq!(options.format, Some(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        assert!(serde_urlencoded::from_str::<EntityWire>("options=normalized%2Cconcise").is_err());
        assert!(serde_urlencoded::from_str::<EntityWire>("options=unknown").is_err());
        assert!(serde_urlencoded::from_str::<EntityWire>("options=sysAttrs%2CsysAttrs").is_err());
    }

    #[test]
    fn temporal_options_are_typed_and_conflicts_are_rejected() {
        let parsed: TemporalWire = serde_urlencoded::from_str("options=temporalValues%2CsysAttrs").expect("valid options");
        let options = parsed.options.expect("options present");
        pretty_assertions::assert_eq!(options.format, Some(TemporalFormat::TemporalValues));
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        assert!(serde_urlencoded::from_str::<TemporalWire>("options=temporalValues%2CaggregatedValues").is_err());
    }

    #[test]
    fn overwrite_options_accept_only_no_overwrite() {
        let parsed: OverwriteWire = serde_urlencoded::from_str("options=noOverwrite").expect("valid option");
        pretty_assertions::assert_eq!(parsed.options, Some(OverwritePolicy::NoOverwrite));
        assert!(serde_urlencoded::from_str::<OverwriteWire>("options=overwrite").is_err());
    }

    #[test]
    fn list_options_are_typed_and_repeated_tokens_are_rejected() {
        let parsed: ListWire = serde_urlencoded::from_str("options=count%2CsysAttrs").expect("valid options");
        let options = parsed.options.expect("options present");
        pretty_assertions::assert_eq!(options.count, CountPolicy::Emit);
        pretty_assertions::assert_eq!(options.sys_attrs, SysAttrsInclusion::Include);
        assert!(serde_urlencoded::from_str::<ListWire>("options=count%2Ccount").is_err());
        assert!(serde_urlencoded::from_str::<ListWire>("options=unknown").is_err());
        assert!(serde_urlencoded::from_str::<ListWire>("options=").is_err());
    }
}
