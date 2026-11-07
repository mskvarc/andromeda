//! The ngsi-ld tree, its `query` area, its `entity_type` part: one test per scenario of the broker's NGSI-LD behaviour,
//! each a sequence of requests and the requirements on their answers, in the shared default tenant unless the test
//! names a private one.

mod n_qry_typ_0001;
mod n_qry_typ_0002;
mod n_qry_typ_0003;
mod n_qry_typ_3001;
mod n_qry_typ_3002;
mod n_qry_typ_9001;
mod n_qry_typ_9004;
mod o_qry_typ_9006;
