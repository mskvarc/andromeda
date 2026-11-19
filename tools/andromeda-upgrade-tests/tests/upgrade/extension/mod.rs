//! The extensions of a tenant database whose versions lag the ones the server
//! ships, as an image upgrade leaves them, or that the catalogue records in a
//! state no update reaches.

mod below_floor;
mod held_update;
mod owner_only_update;
mod repack_recreation;
mod unreadable_version;
