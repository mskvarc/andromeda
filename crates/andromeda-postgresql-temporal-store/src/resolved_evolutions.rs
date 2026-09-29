//! The Temporal Evolutions one chunk's catalogue write resolved its entities
//! to, and which of them the chunk itself brought into being.
//!
//! An evolution the chunk created owns no stream yet, and no other transaction
//! can give it one: its narrow key was minted by this transaction's insert, and
//! every stream references its evolution through an immediate foreign key. Its
//! streams are therefore written without conflict arbitration, while the streams
//! of an evolution that already existed may already be there.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
use std::collections::{HashMap, HashSet};

/// Whether a chunk found a Temporal Evolution or brought it into being.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvolutionOrigin {
    /// This chunk's upsert inserted the evolution, so it has no stream yet.
    Created,
    /// The evolution existed before this chunk, so its streams may too.
    Existing,
}

/// The narrow key each chunk entity resolved to, and the origin of each.
#[derive(Debug, Default)]
pub(crate) struct ResolvedEvolutions {
    /// Narrow key each chunk entity resolved to.
    keys: HashMap<Identifier, TemporalEntityKey>,
    /// The evolutions this chunk's upsert inserted rather than found.
    created: HashSet<TemporalEntityKey>,
}

impl ResolvedEvolutions {
    /// Record that `id` resolved to `key`, found or created as `origin` says.
    pub(crate) fn record(&mut self, id: Identifier, key: TemporalEntityKey, origin: EvolutionOrigin) {
        match origin {
            EvolutionOrigin::Created => {
                self.created.insert(key);
            }
            EvolutionOrigin::Existing => {}
        }
        self.keys.insert(id, key);
    }

    /// The narrow key each chunk entity resolved to.
    pub(crate) const fn keys(&self) -> &HashMap<Identifier, TemporalEntityKey> {
        &self.keys
    }

    /// Whether this chunk found the evolution `key` names or created it.
    pub(crate) fn origin(&self, key: TemporalEntityKey) -> EvolutionOrigin {
        if self.created.contains(&key) {
            EvolutionOrigin::Created
        } else {
            EvolutionOrigin::Existing
        }
    }

    /// The narrow key each chunk entity resolved to, owned.
    pub(crate) fn into_keys(self) -> HashMap<Identifier, TemporalEntityKey> {
        self.keys
    }
}

#[cfg(test)]
mod tests {
    use crate::resolved_evolutions::{EvolutionOrigin, ResolvedEvolutions};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;

    fn key(raw: i64) -> TemporalEntityKey {
        TemporalEntityKey::try_from(raw).expect("a positive narrow key")
    }

    #[test]
    fn an_evolution_keeps_the_origin_it_was_recorded_with() {
        let mut resolved = ResolvedEvolutions::default();
        resolved.record(
            "urn:ngsi-ld:Vehicle:new".parse::<Identifier>().expect("valid URI"),
            key(1),
            EvolutionOrigin::Created,
        );
        resolved.record(
            "urn:ngsi-ld:Vehicle:old".parse::<Identifier>().expect("valid URI"),
            key(2),
            EvolutionOrigin::Existing,
        );
        pretty_assertions::assert_eq!(resolved.origin(key(1)), EvolutionOrigin::Created);
        pretty_assertions::assert_eq!(resolved.origin(key(2)), EvolutionOrigin::Existing);
        pretty_assertions::assert_eq!(resolved.keys().len(), 2);
    }

    /// A key no upsert returned (one a lookup resolved) was found, never created.
    #[test]
    fn an_evolution_the_chunk_did_not_record_as_created_is_existing() {
        pretty_assertions::assert_eq!(ResolvedEvolutions::default().origin(key(9)), EvolutionOrigin::Existing);
    }
}
