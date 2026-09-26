//! The Table 6.5.3.4-1 parameters a clause 5.6.17 merge is performed under.

use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;

/// How one Merge Entity request asked for its fragment to be applied.
///
/// Every member is optional on the wire, so the default is the plain merge of
/// clause 5.6.17: a normalized fragment, no language key, no fallback
/// `observedAt`, and no type selector on the target.
#[derive(Debug, Clone, Copy, Default)]
pub struct MergeOptions<'a> {
    /// The representation the fragment is written in. A simplified fragment
    /// carries bare values whose attribute types the target entity fixes, and
    /// may not change any of them.
    pub representation: MergeRepresentation,
    /// The `languageMap` key a string merged into a pre-existing
    /// `LanguageProperty` updates.
    pub lang: Option<&'a LangTagBuf>,
    /// The `observedAt` a merged instance takes when the fragment names none
    /// and the instance it merges into previously carried one.
    pub observed_at: Option<DateTime<Utc>>,
    /// The clause 4.17 selector the target entity's types must satisfy; a
    /// target outside it is not the entity the request names.
    pub type_selector: Option<&'a TypeExprRaw>,
}

impl MergeOptions<'_> {
    /// Whether the fragment has to be rewritten against the stored entity
    /// before it is merged: a simplified fragment takes the attribute types the
    /// target fixes, and `lang` keys a string into a `LanguageProperty`
    /// (Table 6.5.3.4-1). A normalized fragment without `lang` is merged as
    /// written.
    #[must_use]
    pub const fn rewrites_fragment(&self) -> bool {
        !matches!(self.representation, MergeRepresentation::Normalized) || self.lang.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::merge_options::MergeOptions;
    use andromeda_ngsi_ld_representation::merge_representation::MergeRepresentation;
    use langtag::LangTagBuf;

    #[test]
    fn only_a_normalized_fragment_without_lang_is_merged_as_written() {
        assert!(!MergeOptions::default().rewrites_fragment());
        let lang: LangTagBuf = "en".parse().expect("a valid language tag");
        let keyed = MergeOptions {
            lang: Some(&lang),
            ..MergeOptions::default()
        };
        assert!(keyed.rewrites_fragment());
        let simplified = MergeOptions {
            representation: MergeRepresentation::Simplified,
            ..MergeOptions::default()
        };
        assert!(simplified.rewrites_fragment());
    }
}
