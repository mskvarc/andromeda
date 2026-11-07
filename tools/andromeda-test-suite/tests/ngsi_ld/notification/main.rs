//! The ngsi-ld tree, its `notification` area: one test per scenario of the broker's NGSI-LD behaviour, each a sequence
//! of requests and the requirements on their answers, in the shared default tenant unless the test names a private one.

mod delivery;
mod payload;
mod status;
mod triggering;
