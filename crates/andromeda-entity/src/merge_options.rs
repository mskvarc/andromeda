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
