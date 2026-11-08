//! The language a subscription's notifications are written in (Table 5.2.12-1
//! `lang`, clause 4.15).

use andromeda_http_headers::language::LanguageFilter;
use langtag::LangTagBuf;

/// The clause 4.15 language filter a subscription's `lang` states: every
/// top-level `LanguageProperty` of a notified entity is reduced to it, as a
/// query's `lang` parameter reduces the entities it answers with.
#[derive(Debug, Clone)]
pub struct NotificationLanguage(LanguageFilter);

impl NotificationLanguage {
    /// The filter the subscription's `lang` tag states.
    #[must_use]
    pub fn of(tag: &LangTagBuf) -> NotificationLanguage {
        NotificationLanguage(LanguageFilter::from_tag(tag.clone()))
    }

    /// The filter the entity walk negotiates against.
    pub(crate) const fn filter(&self) -> &LanguageFilter {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_language::NotificationLanguage;
    use langtag::LangTagBuf;

    #[test]
    fn the_filter_ranks_the_one_tag_the_subscription_names() {
        let tag: LangTagBuf = "de".parse().expect("a language tag");
        let language = NotificationLanguage::of(&tag);
        pretty_assertions::assert_eq!(language.filter().preferences().len(), 1);
    }
}
