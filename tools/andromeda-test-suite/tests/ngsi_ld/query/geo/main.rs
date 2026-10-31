//! The ngsi-ld tree, its `query` area, its `geo` part: one test per scenario of the broker's NGSI-LD behaviour, each a
//! sequence of requests and the requirements on their answers, in the shared default tenant unless the test names a
//! private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_qry_geo_0001;
mod n_qry_geo_0002;
mod n_qry_geo_0003;
mod n_qry_geo_0004;
mod n_qry_geo_0005;
mod n_qry_geo_0006;
mod n_qry_geo_0007;
mod n_qry_geo_0008;
mod n_qry_geo_0009;
mod n_qry_geo_0010;
mod n_qry_geo_0012;
mod n_qry_geo_0013;
mod n_qry_geo_0014;
mod n_qry_geo_0015;
mod n_qry_geo_0016;
mod n_qry_geo_0017;
mod n_qry_geo_0018;
mod n_qry_geo_0019;
mod n_qry_geo_0027;
mod n_qry_geo_0028;
mod n_qry_geo_4001;
mod n_qry_geo_4002;
mod n_qry_geo_9001;
mod n_qry_geo_9002;
mod n_qry_geo_9003;
mod n_qry_geo_9004;
mod n_qry_geo_9005;
mod n_qry_geo_9006;
mod n_qry_geo_9007;
mod n_qry_geo_9008;
mod n_qry_geo_9009;
mod n_qry_geo_9010;
mod n_qry_geo_9012;
mod n_qry_geo_9090;
mod n_qry_geo_9091;
mod n_qry_geo_9092;
mod o_qry_geo_9011;
mod o_qry_geo_9014;
mod round_trip;
mod three_dimensional;
