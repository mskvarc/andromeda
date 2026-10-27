//! The language map of a `LanguageProperty` (clause 4.5.18).
//!
//! Expansion under the `@language` container of `languageMap` turns each map
//! entry into a value object carrying the text and its `@language`; the
//! `@none` entry carries no language, and a language with several strings
//! yields one value object per string. The entries are kept in that expanded
//! order. Language tags compare case-insensitively (IETF RFC 5646 section
//! 2.1.1), so `EN` and `en` name one language.

use crate::{
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentText, Stored, ValueMode},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use langtag::LangTagBuf;

/// One entry of a language map.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageEntry<M: ValueMode = Stored> {
    /// The language, or `None` for the `@none` entry.
    language: Option<LangTagBuf>,
    /// The text in that language.
    text: M::Text,
}

impl<M: ValueMode> LanguageEntry<M> {
    /// One entry.
    #[must_use]
    pub const fn new(language: Option<LangTagBuf>, text: M::Text) -> LanguageEntry<M> {
        LanguageEntry { language, text }
    }

    /// The language, or `None` for the `@none` entry.
    #[must_use]
    pub const fn language(&self) -> Option<&LangTagBuf> {
        self.language.as_ref()
    }

    /// The text.
    #[must_use]
    pub const fn text(&self) -> &M::Text {
        &self.text
    }

    /// Give the entry up as its parts.
    #[must_use]
    pub fn into_parts(self) -> (Option<LangTagBuf>, M::Text) {
        (self.language, self.text)
    }
}

/// The entries of one language map, in expanded order.
#[derive(Clone, Debug, PartialEq)]
pub struct LanguageEntries<M: ValueMode = Stored>(Vec<LanguageEntry<M>>);

impl<M: ValueMode> LanguageEntries<M> {
    /// Hold `entries` in order.
    #[must_use]
    pub const fn new(entries: Vec<LanguageEntry<M>>) -> LanguageEntries<M> {
        LanguageEntries(entries)
    }

    /// The entries, in order.
    #[must_use]
    pub fn entries(&self) -> &[LanguageEntry<M>] {
        &self.0
    }

    /// The entries, for a mutation.
    pub const fn entries_mut(&mut self) -> &mut Vec<LanguageEntry<M>> {
        &mut self.0
    }

    /// Give the entries up.
    #[must_use]
    pub fn into_entries(self) -> Vec<LanguageEntry<M>> {
        self.0
    }

    /// Whether the map holds no entry.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl LanguageEntries<Fragment> {
    /// Hold the written `entries` of `attribute`'s language map.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::NullBesideData`] when one language holds the
    /// NGSI-LD Null beside a string: the Null deletes the language
    /// (clause 5.5.12) and a string writes it, so the two cannot share it.
    pub fn written(attribute: &AttributeName, entries: Vec<LanguageEntry<Fragment>>) -> Result<LanguageEntries<Fragment>, EntityRefusal> {
        let deletes_beside_text = entries.iter().any(|deletion| {
            matches!(deletion.text(), FragmentText::Remove)
                && entries
                    .iter()
                    .any(|text| text.language() == deletion.language() && matches!(text.text(), FragmentText::Text(_)))
        });
        if deletes_beside_text {
            return Err(EntityRefusal::NullBesideData {
                attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
            });
        }
        Ok(LanguageEntries(entries))
    }
}
