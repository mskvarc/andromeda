//! The tokens an `options` parameter can carry.

use strum::{Display, EnumString};

/// One `options` token, spelled as the specification spells it.
///
/// This is the union of the tokens every operation's `options` grammar draws
/// on (the renderings of Tables 6.3.7-1 and 6.3.12-1, `sysAttrs` (clause
/// 6.3.11), `noOverwrite` (clause 6.3.9) and the two upsert modes of clause
/// 6.15.3.1). Which of them one operation accepts is that operation's grammar;
/// a token outside the union names nothing at all. The total of a paged query
/// is the `count` URL parameter of clause 6.3.13, not a token.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, PartialEq)]
#[strum(serialize_all = "camelCase")]
pub enum OptionsToken {
    /// The normalized entity rendering.
    Normalized,
    /// The concise entity rendering.
    Concise,
    /// The simplified entity rendering.
    Simplified,
    /// The deprecated synonym of `simplified`.
    KeyValues,
    /// Include the system-generated temporal properties.
    SysAttrs,
    /// The simplified temporal rendering.
    TemporalValues,
    /// The aggregated temporal rendering.
    AggregatedValues,
    /// The grouped temporal rendering of the temporal aggregation grouping
    /// extension.
    GroupedValues,
    /// Preserve existing attributes on append and batch update.
    NoOverwrite,
    /// Replace an existing entity on batch upsert.
    Replace,
    /// Update an existing entity on batch upsert.
    Update,
}

#[cfg(test)]
mod tests {
    use crate::options_token::OptionsToken;

    #[test]
    fn a_token_round_trips_through_its_specification_spelling() {
        for (token, spelling) in [
            (OptionsToken::KeyValues, "keyValues"),
            (OptionsToken::SysAttrs, "sysAttrs"),
            (OptionsToken::NoOverwrite, "noOverwrite"),
            (OptionsToken::GroupedValues, "groupedValues"),
        ] {
            pretty_assertions::assert_eq!(token.to_string(), spelling);
            pretty_assertions::assert_eq!(spelling.parse::<OptionsToken>(), Ok(token));
        }
    }

    #[test]
    fn a_token_is_matched_case_sensitively() {
        assert!("KEYVALUES".parse::<OptionsToken>().is_err());
    }
}
