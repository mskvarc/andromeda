//! The ngsi-ld tree, its `query` area, its `order` part: one test per scenario of the broker's NGSI-LD behaviour, each
//! a sequence of requests and the requirements on their answers, in the shared default tenant unless the test names a
//! private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod n_qry_ord_0001;
mod n_qry_ord_0002;
mod n_qry_ord_0003;
mod n_qry_ord_0004;
mod n_qry_ord_0005;
mod n_qry_ord_0006;
mod n_qry_ord_0007;
mod n_qry_ord_0009;
mod n_qry_ord_0011;
mod n_qry_ord_0012;
mod n_qry_ord_0013;
mod n_qry_ord_0016;
mod n_qry_ord_0017;
mod n_qry_ord_3001;
mod n_qry_ord_4001;
mod n_qry_ord_4002;
mod n_qry_ord_4003;
mod n_qry_ord_4004;
mod n_qry_ord_4005;
mod n_qry_ord_4006;
mod n_qry_ord_4007;
mod n_qry_ord_4008;
mod n_qry_ord_9004;
mod n_qry_ord_9005;
mod n_qry_ord_9006;
mod n_qry_ord_9008;
mod n_qry_ord_9009;
mod n_qry_ord_9010;
mod n_qry_ord_9011;
mod n_qry_ord_9012;
mod n_qry_ord_9014;
mod n_qry_ord_9015;
mod n_qry_ord_9016;
mod n_qry_ord_9017;
mod n_qry_ord_9018;
mod n_qry_ord_9020;
mod n_qry_ord_9021;
mod n_qry_ord_9022;
mod n_qry_ord_9026;
mod n_qry_ord_9030;
mod n_qry_ord_9396;
mod o_qry_ord_0008;
mod o_qry_ord_0010;
mod o_qry_ord_0014;
mod o_qry_ord_0015;
mod o_qry_ord_9001;
mod o_qry_ord_9002;
mod o_qry_ord_9003;
mod o_qry_ord_9007;
mod o_qry_ord_9019;
mod o_qry_ord_9022;
mod o_qry_ord_9023;
mod o_qry_ord_9024;
mod o_qry_ord_9025;
mod o_qry_ord_9027;
mod o_qry_ord_9028;
mod o_qry_ord_9029;
