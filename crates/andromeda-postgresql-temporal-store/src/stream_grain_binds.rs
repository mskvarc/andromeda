//! The stream grains one chunk of writes requests, column-major for the
//! catalogue's `UNNEST` statements and indexed so that every row either
//! statement returns maps back to the grain it answers for.
//!
//! A row carries only stored keys (`temporal_entity_key`, `attribute_name_key`,
//! `dataset_id`); the index turns them back into the requested grain by bind
//! position, so no attribute name or dataset identifier is ever re-read or
//! re-parsed from the database.

use crate::{
    error::TemporalStoreError,
    resolved_evolutions::{EvolutionOrigin, ResolvedEvolutions},
    stored_catalog_error::StoredCatalogError,
    temporal_store_invariant::TemporalStoreInvariant,
    temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_codes::{stream_key::StreamKey, temporal_entity_key::TemporalEntityKey};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use std::collections::{HashMap, HashSet};

/// One stream row a catalogue statement returns:
/// `(temporal_entity_key, attribute_name_key, dataset_id, stream_key)`.
pub(crate) type StreamRow = (i64, i32, Option<String>, i64);

/// The stream each requested grain resolved to, keyed by the entity it belongs to.
pub(crate) type ResolvedStreams = HashMap<(TemporalEntityKey, TemporalStreamKey), StreamKey>;

/// The stored identity of one grain, as a statement binds and returns it.
type StoredGrain<'a> = (i64, i32, Option<&'a str>);

/// The column-major arrays one catalogue statement binds, one row per grain.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct GrainColumns<'a> {
    /// Narrow entity key each stream belongs to.
    pub(crate) temporal_entity_keys: Vec<i64>,
    /// Dictionary identity of each stream's attribute name.
    pub(crate) attribute_name_keys: Vec<i32>,
    /// Multi-attribute discriminator of each stream.
    pub(crate) dataset_ids: Vec<Option<&'a str>>,
}

impl<'a> GrainColumns<'a> {
    /// Whether the statement would bind no grain at all.
    pub(crate) const fn is_empty(&self) -> bool {
        self.temporal_entity_keys.is_empty()
    }

    /// Append one grain.
    fn push(&mut self, (temporal_entity_key, attribute_name_key, dataset_id): StoredGrain<'a>) {
        self.temporal_entity_keys.push(temporal_entity_key);
        self.attribute_name_keys.push(attribute_name_key);
        self.dataset_ids.push(dataset_id);
    }
}

/// What the catalogue statements have resolved so far: the streams, and the
/// bind positions they answer for.
#[derive(Debug, Default)]
pub(crate) struct GrainResolution {
    /// The stream of every resolved grain.
    streams: ResolvedStreams,
    /// The bind positions already resolved.
    positions: HashSet<usize>,
}

impl GrainResolution {
    /// The resolved streams, once every grain has one.
    pub(crate) fn into_streams(self) -> ResolvedStreams {
        self.streams
    }
}

/// Every stream grain one chunk requests, deduplicated to the full
/// `(entity, attribute, datasetId)` grain.
#[derive(Debug)]
pub(crate) struct StreamGrainBinds<'a> {
    /// The grains of evolutions this chunk created, which own no stream yet.
    created: GrainColumns<'a>,
    /// The grains of evolutions that existed before this chunk.
    existing: GrainColumns<'a>,
    /// The requested grain at each position.
    grains: Vec<(TemporalEntityKey, &'a TemporalStreamKey)>,
    /// The stored identity of the grain at each position.
    stored: Vec<StoredGrain<'a>>,
    /// The position of each stored grain.
    positions: HashMap<StoredGrain<'a>, usize>,
}

impl<'a> StreamGrainBinds<'a> {
    /// Spread every entity's requested grains into column-major binds, one row
    /// per distinct grain, carrying the grain's entity key so one statement
    /// spans the whole chunk, and split by whether the chunk created the
    /// grain's evolution.
    ///
    /// Two different entities restating one attribute yield two rows, while one
    /// entity restating a grain yields one: a write may carry several
    /// observations of one attribute, and they all extend the same stream. An
    /// upsert without a temporal entity key owns no evolution and requests no
    /// stream: it is a deletion whose evolution clause 5.6.16 already removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when a grain names an
    /// attribute the dictionary did not resolve.
    pub(crate) fn new(
        upserts: &'a [TemporalCatalogUpsert],
        evolutions: &ResolvedEvolutions,
        name_to_id: &HashMap<AttributeName, AttributeNameKey>,
    ) -> Result<StreamGrainBinds<'a>, TemporalStoreError> {
        let mut binds = StreamGrainBinds {
            created: GrainColumns::default(),
            existing: GrainColumns::default(),
            grains: Vec::new(),
            stored: Vec::new(),
            positions: HashMap::new(),
        };
        for upsert in upserts {
            let Some(entity_key) = evolutions.keys().get(&upsert.entity_id) else {
                continue;
            };
            let columns = match evolutions.origin(*entity_key) {
                EvolutionOrigin::Created => &mut binds.created,
                EvolutionOrigin::Existing => &mut binds.existing,
            };
            for grain in &upsert.streams {
                // clone: the invariant owns the name the dictionary failed to resolve.
                let attribute_name_key = *name_to_id
                    .get(&grain.attribute_name)
                    .ok_or_else(|| TemporalStoreInvariant::AttributeNameUnresolved(grain.attribute_name.clone()))?;
                let stored = (entity_key.get(), attribute_name_key.get(), grain.dataset_id.as_ref().map(Identifier::as_str));
                if binds.positions.contains_key(&stored) {
                    continue;
                }
                binds.positions.insert(stored, binds.grains.len());
                binds.grains.push((*entity_key, grain));
                binds.stored.push(stored);
                columns.push(stored);
            }
        }
        Ok(binds)
    }

    /// Whether the chunk requests no stream at all.
    pub(crate) const fn is_empty(&self) -> bool {
        self.grains.is_empty()
    }

    /// The grains of `origin` evolutions, as the arrays an insert binds.
    pub(crate) const fn of(&self, origin: EvolutionOrigin) -> &GrainColumns<'a> {
        match origin {
            EvolutionOrigin::Created => &self.created,
            EvolutionOrigin::Existing => &self.existing,
        }
    }

    /// Record the stream of every row a catalogue statement returned against
    /// the grain it answers for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::StoredCatalog`] when a row carries an
    /// invalid stream key, and [`TemporalStoreError::Invariant`] when it answers
    /// for a grain this chunk never requested.
    pub(crate) fn resolve(&self, resolution: &mut GrainResolution, rows: Vec<StreamRow>) -> Result<(), TemporalStoreError> {
        for (temporal_entity_key, attribute_name_key, dataset_id, stream_key) in rows {
            let position = *self
                .positions
                .get(&(temporal_entity_key, attribute_name_key, dataset_id.as_deref()))
                .ok_or(TemporalStoreInvariant::UnrequestedStream { stream_key })?;
            let (entity_key, grain) = self
                .grains
                .get(position)
                .copied()
                .ok_or(TemporalStoreInvariant::GrainPositionOutOfRange { position })?;
            let stream_key = StreamKey::try_from(stream_key).map_err(StoredCatalogError::StreamKey)?;
            // clone: the resolved map outlives the chunk's borrowed grains.
            resolution.streams.insert((entity_key, grain.clone()), stream_key);
            resolution.positions.insert(position);
        }
        Ok(())
    }

    /// The grains no statement has resolved yet, as the arrays a read-back binds.
    pub(crate) fn unresolved(&self, resolution: &GrainResolution) -> GrainColumns<'a> {
        let mut pending = GrainColumns::default();
        self.stored
            .iter()
            .enumerate()
            .filter(|(position, _)| !resolution.positions.contains(position))
            .for_each(|(_, stored)| pending.push(*stored));
        pending
    }

    /// The first requested grain no statement resolved, if any.
    pub(crate) fn first_missing(&self, resolution: &GrainResolution) -> Option<&'a TemporalStreamKey> {
        self.grains
            .iter()
            .enumerate()
            .find(|(position, _)| !resolution.positions.contains(position))
            .map(|(_, (_, grain))| *grain)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        resolved_evolutions::{EvolutionOrigin, ResolvedEvolutions},
        stream_grain_binds::{GrainResolution, StreamGrainBinds},
        temporal_stream::{TemporalCatalogUpsert, TemporalStreamKey},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_postgresql_codes::temporal_entity_key::TemporalEntityKey;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use andromeda_temporal::write_purpose::TemporalWritePurpose;
    use std::collections::HashMap;

    const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND: &str = "https://uri.etsi.org/ngsi-ld/default-context/brand";

    fn grain(attribute_name: &str, dataset_id: Option<&str>) -> TemporalStreamKey {
        TemporalStreamKey {
            attribute_name: attribute_name.parse::<AttributeName>().expect("valid attribute name"),
            dataset_id: dataset_id.map(|raw| raw.parse::<Identifier>().expect("valid dataset URI")),
        }
    }

    fn upsert(entity_id: &str, streams: Vec<TemporalStreamKey>) -> TemporalCatalogUpsert {
        TemporalCatalogUpsert {
            entity_id: entity_id.parse::<Identifier>().expect("valid entity URI"),
            types: Vec::new(),
            purpose: TemporalWritePurpose::Observation,
            streams,
        }
    }

    fn name_keys() -> HashMap<AttributeName, AttributeNameKey> {
        HashMap::from([
            (SPEED.parse().expect("valid speed name"), AttributeNameKey::from(1_i32)),
            (BRAND.parse().expect("valid brand name"), AttributeNameKey::from(2_i32)),
        ])
    }

    fn key(raw: i64) -> TemporalEntityKey {
        TemporalEntityKey::try_from(raw).expect("a positive narrow key")
    }

    fn evolutions(ids: &[(&str, i64, EvolutionOrigin)]) -> ResolvedEvolutions {
        let mut resolved = ResolvedEvolutions::default();
        for (id, raw, origin) in ids {
            resolved.record(id.parse::<Identifier>().expect("valid entity URI"), key(*raw), *origin);
        }
        resolved
    }

    #[test]
    fn chunk_dedup_keeps_two_entities_sharing_one_attribute_as_two_streams() {
        let upserts = [
            upsert("urn:ngsi-ld:Vehicle:1", vec![grain(SPEED, None), grain(SPEED, None)]),
            upsert("urn:ngsi-ld:Vehicle:2", vec![grain(SPEED, None)]),
        ];
        let resolved = evolutions(&[
            ("urn:ngsi-ld:Vehicle:1", 1, EvolutionOrigin::Existing),
            ("urn:ngsi-ld:Vehicle:2", 2, EvolutionOrigin::Existing),
        ]);
        let binds = StreamGrainBinds::new(&upserts, &resolved, &name_keys()).expect("known names");
        let existing = binds.of(EvolutionOrigin::Existing);
        pretty_assertions::assert_eq!(existing.temporal_entity_keys, vec![1_i64, 2_i64]);
        pretty_assertions::assert_eq!(existing.attribute_name_keys, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(existing.dataset_ids, vec![None, None]);
        assert!(binds.of(EvolutionOrigin::Created).is_empty());
    }

    #[test]
    fn one_attribute_under_two_datasets_binds_two_streams_for_one_entity() {
        let upserts = [upsert(
            "urn:ngsi-ld:Vehicle:1",
            vec![grain(SPEED, None), grain(SPEED, Some("urn:ngsi-ld:Dataset:a"))],
        )];
        let resolved = evolutions(&[("urn:ngsi-ld:Vehicle:1", 7, EvolutionOrigin::Existing)]);
        let binds = StreamGrainBinds::new(&upserts, &resolved, &name_keys()).expect("known names");
        let existing = binds.of(EvolutionOrigin::Existing);
        pretty_assertions::assert_eq!(existing.temporal_entity_keys, vec![7_i64, 7_i64]);
        pretty_assertions::assert_eq!(existing.attribute_name_keys, vec![1_i32, 1_i32]);
        pretty_assertions::assert_eq!(existing.dataset_ids, vec![None, Some("urn:ngsi-ld:Dataset:a")]);
    }

    /// The grains of an evolution the chunk created go to the insert without
    /// conflict arbitration; those of one that already existed do not.
    #[test]
    fn the_grains_of_a_created_evolution_are_bound_apart_from_an_existing_ones() {
        let upserts = [
            upsert("urn:ngsi-ld:Vehicle:new", vec![grain(SPEED, None), grain(BRAND, None)]),
            upsert("urn:ngsi-ld:Vehicle:old", vec![grain(SPEED, None)]),
        ];
        let resolved = evolutions(&[
            ("urn:ngsi-ld:Vehicle:new", 1, EvolutionOrigin::Created),
            ("urn:ngsi-ld:Vehicle:old", 2, EvolutionOrigin::Existing),
        ]);
        let binds = StreamGrainBinds::new(&upserts, &resolved, &name_keys()).expect("known names");
        pretty_assertions::assert_eq!(binds.of(EvolutionOrigin::Created).temporal_entity_keys, vec![1_i64, 1_i64]);
        pretty_assertions::assert_eq!(binds.of(EvolutionOrigin::Created).attribute_name_keys, vec![1_i32, 2_i32]);
        pretty_assertions::assert_eq!(binds.of(EvolutionOrigin::Existing).temporal_entity_keys, vec![2_i64]);
    }

    #[test]
    fn an_upsert_without_an_evolution_requests_no_stream() {
        let upserts = [upsert("urn:ngsi-ld:Vehicle:gone", vec![grain(SPEED, None)])];
        let binds = StreamGrainBinds::new(&upserts, &ResolvedEvolutions::default(), &name_keys()).expect("known names");
        assert!(binds.is_empty());
    }

    /// The insert returns only the streams it created; the read-back binds
    /// exactly the grains it did not, and each returned row lands on the grain
    /// it answers for, however the rows are ordered.
    #[test]
    fn only_the_grains_the_insert_did_not_return_are_read_back() {
        let upserts = [
            upsert("urn:ngsi-ld:Vehicle:1", vec![grain(SPEED, None), grain(BRAND, Some("urn:ngsi-ld:Dataset:a"))]),
            upsert("urn:ngsi-ld:Vehicle:2", vec![grain(SPEED, None)]),
        ];
        let resolved = evolutions(&[
            ("urn:ngsi-ld:Vehicle:1", 1, EvolutionOrigin::Existing),
            ("urn:ngsi-ld:Vehicle:2", 2, EvolutionOrigin::Existing),
        ]);
        let binds = StreamGrainBinds::new(&upserts, &resolved, &name_keys()).expect("known names");
        let mut resolution = GrainResolution::default();
        binds
            .resolve(&mut resolution, vec![(2, 1, None, 30), (1, 2, Some("urn:ngsi-ld:Dataset:a".to_owned()), 20)])
            .expect("requested grains");
        let pending = binds.unresolved(&resolution);
        pretty_assertions::assert_eq!(pending.temporal_entity_keys, vec![1_i64]);
        pretty_assertions::assert_eq!(pending.attribute_name_keys, vec![1_i32]);
        pretty_assertions::assert_eq!(pending.dataset_ids, vec![None]);
        pretty_assertions::assert_eq!(binds.first_missing(&resolution), Some(&upserts[0].streams[0]));

        binds.resolve(&mut resolution, vec![(1, 1, None, 10)]).expect("requested grain");
        assert!(binds.unresolved(&resolution).is_empty());
        assert!(binds.first_missing(&resolution).is_none());
        let streams = resolution.into_streams();
        pretty_assertions::assert_eq!(streams.get(&(key(1), grain(SPEED, None))).map(|stream| stream.get()), Some(10));
        pretty_assertions::assert_eq!(
            streams.get(&(key(1), grain(BRAND, Some("urn:ngsi-ld:Dataset:a")))).map(|stream| stream.get()),
            Some(20)
        );
        pretty_assertions::assert_eq!(streams.get(&(key(2), grain(SPEED, None))).map(|stream| stream.get()), Some(30));
    }

    #[test]
    fn a_row_for_a_grain_the_chunk_never_requested_is_refused() {
        let upserts = [upsert("urn:ngsi-ld:Vehicle:1", vec![grain(SPEED, None)])];
        let resolved = evolutions(&[("urn:ngsi-ld:Vehicle:1", 1, EvolutionOrigin::Created)]);
        let binds = StreamGrainBinds::new(&upserts, &resolved, &name_keys()).expect("known names");
        let mut resolution = GrainResolution::default();
        assert!(binds.resolve(&mut resolution, vec![(1, 2, None, 10)]).is_err());
    }
}
