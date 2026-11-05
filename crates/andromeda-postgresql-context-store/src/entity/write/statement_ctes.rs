//! The common table expressions the entity set-writes name.
//!
//! Every statement projects its staging once into `proj`, writes the identity
//! rows first, and drives every other relation it writes from the identity
//! rows that write landed; the names here are how the statements and the
//! relation writes they share refer to one another.

use sea_query::Iden;

/// The projection CTE, `proj`: one row per staged entity, typed as stored.
#[derive(Iden, Clone, Copy)]
#[iden = "proj"]
pub(super) struct Proj;

/// The identity write of an insert or an update, `stamped`: every identity
/// row the write met, with the latest instant of each Entity whose write it
/// refused for holding a later one.
#[derive(Iden, Clone, Copy)]
#[iden = "stamped"]
pub(super) struct Stamped;

/// The identity rows an insert landed, `ins`, whose keys drive the child
/// inserts.
#[derive(Iden, Clone, Copy)]
#[iden = "ins"]
pub(super) struct Ins;

/// The column of `ins` saying whether the create minted the identity row with
/// the Temporal Evolution it was asked to establish, `established`: false for
/// an identity row that existed, which keeps the evolution it held.
#[derive(Iden, Clone, Copy)]
#[iden = "established"]
pub(super) struct Established;

/// The identity rows an update landed, `ents`: the rows of the update's
/// compare-and-set anchor it wrote, whose keys drive every other relation the
/// update writes.
#[derive(Iden, Clone, Copy)]
#[iden = "ents"]
pub(super) struct Ents;

/// The `entity_search` CTE, `srch`: the rows a set-write wrote there, returned
/// as the images ([`SearchImage`](crate::entity::write::search_image::SearchImage))
/// the memberships and the census are derived from.
#[derive(Iden, Clone, Copy)]
#[iden = "srch"]
pub(crate) struct Srch;

/// The `current_attributes` CTE, `attrs`: the attribute instance rows a
/// set-write lands.
#[derive(Iden, Clone, Copy)]
#[iden = "attrs"]
pub(super) struct Attrs;

/// The update's `current_attributes` delete CTE, `gone`: the stored instance
/// rows of an overwritten entity whose grain the written state no longer
/// holds.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
pub(super) struct Gone;

/// The update's `current_attributes` delete CTE, `dropped`: the stored
/// instance rows whose grain an edit removes.
#[derive(Iden, Clone, Copy)]
#[iden = "dropped"]
pub(super) struct Dropped;

/// The update's `current_attributes` update CTE, `moved`: the stored instance
/// rows an edit gives a new ordinal and nothing else.
#[derive(Iden, Clone, Copy)]
#[iden = "moved"]
pub(super) struct Moved;

/// The delete's candidate CTE, `doomed`: the rows about to go, with what the
/// deletion notification needs from them.
#[derive(Iden, Clone, Copy)]
#[iden = "doomed"]
pub(super) struct Doomed;
