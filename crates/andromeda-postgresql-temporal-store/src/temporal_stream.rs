//! The grain one temporal attribute stream is keyed by.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_temporal::write_purpose::TemporalWritePurpose;

/// The natural key of one temporal attribute stream.
///
/// A stream groups every observation of one attribute of one entity under one
/// `datasetId`, which is the grain clause 5.2.5 gives a multi-attribute. The
/// catalogue holds only that grain: the attribute family and the value shape are
/// recorded per history row instead, so an attribute whose type later changes
/// does not retroactively reinterpret what was already stored.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TemporalStreamKey {
    /// Expanded IRI of the attribute the stream records.
    pub attribute_name: AttributeName,
    /// Multi-attribute discriminator, absent for the default instance.
    pub dataset_id: Option<Uri>,
}

/// The catalogue rows one write needs before its history rows can be stored.
///
/// The temporal entity is upserted together with its streams because clause
/// 5.6.6 leaves temporal evolution standing when current state is deleted: the
/// temporal catalogue is not reachable from `entities` and has to be able to
/// come into existence on its own.
#[derive(Clone, Debug)]
pub struct TemporalCatalogUpsert {
    /// The entity whose temporal evolution the write extends.
    pub entity_id: Uri,
    /// Entity types from clause 4.5.1, empty when the write states no entity.
    ///
    /// These are the types of the entity the write produced, which is what
    /// clause 4.5.6 makes the Temporal Evolution be. A write that states none —
    /// a deletion, whose only consequence is the ending of instances — cannot
    /// create the temporal entity, since clause 4.5.1 makes at least one type
    /// part of what an entity is.
    pub types: Vec<EntityTypeName>,
    /// What the write is for, which decides what a missing evolution means.
    ///
    /// A typeless upsert cannot create the temporal entity, and only the
    /// write's purpose says whether that is a failure or a no-op: an
    /// observation has nowhere to land, while a deletion has nothing to end. A
    /// deletion never creates one even when it does state types.
    pub purpose: TemporalWritePurpose,
    /// The stream grains the write's history rows belong to.
    pub streams: Vec<TemporalStreamKey>,
}
