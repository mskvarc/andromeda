//! The ngsi-ld tree, its `attribute` area: one test per scenario of the broker's NGSI-LD behaviour, each a sequence of
//! requests and the requirements on their answers, in the shared default tenant unless the test names a private one.

mod append;
mod delete;
mod partial_update;
mod replace;
mod update;
