//! Persistence layer for NGSI-LD entities.

/// The discovery census the entity statements keep beside the search rows.
pub mod census;
/// Executing a compiled entity query: its page, its total and its identifiers.
pub mod compiled_query;
/// Binary COPY path for bulk entity inserts.
pub mod copy;
/// The reads that let a deletion skip the payload it removes.
pub mod deletion_outline;
/// Reading interned keys back as the names the dictionaries hold.
pub(crate) mod dictionary_join;
/// Reading the types one entity carries from its search row.
pub mod entity_types;
/// Removing entities and attribute instances whose `expiresAt` has passed.
pub mod expiry;
/// The identity the fused entity insert hands back, with its minted key.
pub mod inserted_entity;
/// Resolving surrogate keys of entities a commit touched without writing them.
pub mod keys;
/// Row-to-domain conversions for the `entities` table.
pub mod mapper;
/// The parallel bind vectors every entity statement is driven from.
pub mod params;
/// Reading whole entity payloads a page at a time.
pub mod payload_listing;
/// Direct implementations of entity policy ports.
mod port;
/// Deciding what to ask PostgreSQL for, and what to hand back unanswered.
pub mod pushdown;
/// The statements that read one entity's current state.
pub mod read;
/// The PostgreSQL entity store.
pub mod repo;
/// The Scope label paths one entity's search row is matched by.
pub mod scope_label_paths;
/// How a set-write stages one hoisted geometry of one entity row.
pub mod staged_geometry;
/// What an Entity row or census ledger that no longer reads back looks like.
pub mod stored_entity_error;
/// The clause 4.5.1 invariant that every stored entity carries at least one type.
pub mod types_present;
/// The statements that write the three current-state entity relations.
pub mod write;
/// The identity a write statement hands back for each entity row it affected.
pub mod written_entity;
