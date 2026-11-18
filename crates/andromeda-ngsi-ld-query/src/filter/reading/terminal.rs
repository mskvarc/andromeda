//! What a Query Term's path ends on.

use crate::filter::{instance_metadata::InstanceMetadata, language_matching::LanguageMatching, target_language::TargetLanguage, trailing_group::MemberPath};
use langtag::LangTagBuf;

/// The target a path reaches at the last instance it walks to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Terminal {
    /// The instance's own value, read as its family's reading says.
    OwnValue,
    /// A member the instance carries as data of its own (`observedAt`,
    /// `unitCode`, ...), a non-reified Property with nothing beneath it.
    InstanceMember(InstanceMetadata),
    /// A target language only a tag or a range spells (`marque[en-GB]`), read
    /// on a `LanguageProperty` and nothing on any other family.
    Language {
        /// The target language.
        language: TargetLanguage,
        /// How it selects languageMap keys, the tenant's language-targeting
        /// level.
        matching: LanguageMatching,
    },
    /// A member path into a compound value, in both spellings.
    Members(MemberPath),
    /// One trailing member that is also a language tag (`color[en]`): the
    /// target language on a family whose value is keyed by language, the
    /// compound member on every other.
    ByFamily {
        /// The member read as a target language.
        language: LangTagBuf,
        /// How it selects languageMap keys, the tenant's language-targeting
        /// level.
        matching: LanguageMatching,
        /// The member read as a compound value's member.
        members: MemberPath,
    },
}
