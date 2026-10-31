//! The entities of one source's answer that are left out because they cannot be used (clause 6.3.17).
//!
//! An entity is dropped while a page is decoded or while it is read into the model, and a source's answer runs over several pages and, for a history, several resumed walks. The drops are gathered over all of them, so the source is reported with one `111` warning that says how many of its entities are missing. The warning counts them and names none: what the source wrote stays in the broker's log, which records each dropped entity with the reason.

use crate::source_answer_failure::SourceAnswerFailure;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::collections::HashSet;

/// The entities left out of one source's answer.
#[derive(Debug, Default)]
pub(crate) struct DroppedEntities {
    /// The identifiers of the dropped entities that name a readable one, so an entity the source answers on two pages or two resumed walks is counted once.
    named: HashSet<Identifier>,
    /// How many dropped entities name no readable identifier; each is counted, as none can be told from another.
    unnamed: usize,
}

impl DroppedEntities {
    /// Record one dropped entity, by the identifier it names when it names one.
    pub(crate) fn record(&mut self, id: Option<Identifier>) {
        match id {
            Some(id) => {
                self.named.insert(id);
            }
            None => self.unnamed = self.unnamed.saturating_add(1),
        }
    }

    /// Take over the entities a later page or walk of the same answer dropped.
    pub(crate) fn absorb(&mut self, later: DroppedEntities) {
        self.named.extend(later.named);
        self.unnamed = self.unnamed.saturating_add(later.unnamed);
    }

    /// How many entities were dropped.
    pub(crate) fn count(&self) -> usize {
        self.named.len().saturating_add(self.unnamed)
    }

    /// The one warning reporting the dropped entities of the source `registration` names, `None` when none was dropped.
    pub(crate) fn warning(&self, registration: &Identifier) -> Option<NgsiLdWarning> {
        let count = self.count();
        (count > 0).then(|| SourceAnswerFailure::EntitiesDropped { registration, count }.into_warning())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::dropped_entities::DroppedEntities;
    use andromeda_http_headers::ngsild_warning::WarnCode;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn id(raw: &str) -> Identifier {
        raw.parse().expect("a valid identifier")
    }

    #[test]
    fn nothing_dropped_is_no_warning() {
        assert_eq!(DroppedEntities::default().warning(&id("urn:ngsi-ld:ContextSourceRegistration:1")), None);
    }

    #[test]
    fn every_dropped_entity_is_counted_in_one_revalidation_warning() {
        let mut dropped = DroppedEntities::default();
        dropped.record(Some(id("urn:ngsi-ld:Vehicle:a")));
        dropped.record(Some(id("urn:ngsi-ld:Vehicle:b")));
        dropped.record(None);
        let warning = dropped.warning(&id("urn:ngsi-ld:ContextSourceRegistration:1")).expect("a warning");
        pretty_assertions::assert_eq!(warning.code(), WarnCode::RevalidationFailed);
        pretty_assertions::assert_eq!(
            warning.text().as_str(),
            "3 of the entities context source urn:ngsi-ld:ContextSourceRegistration:1 answered cannot be used"
        );
    }

    #[test]
    fn an_entity_dropped_on_two_pages_is_counted_once_and_each_unnamed_one_every_time() {
        let mut first = DroppedEntities::default();
        first.record(Some(id("urn:ngsi-ld:Vehicle:a")));
        first.record(None);
        let mut later = DroppedEntities::default();
        later.record(Some(id("urn:ngsi-ld:Vehicle:a")));
        later.record(Some(id("urn:ngsi-ld:Vehicle:b")));
        later.record(None);
        first.absorb(later);
        pretty_assertions::assert_eq!(first.count(), 4);
    }

    /// The warning counts the dropped entities and repeats nothing the source wrote.
    #[test]
    fn the_warning_names_no_dropped_entity() {
        let mut dropped = DroppedEntities::default();
        dropped.record(Some(id("urn:ngsi-ld:Vehicle:secret")));
        let warning = dropped.warning(&id("urn:ngsi-ld:ContextSourceRegistration:1")).expect("a warning");
        assert!(!warning.text().as_str().contains("secret"), "{warning}");
    }
}
