//! Loop detection: the `Via` header a broker sends with a forwarded request, and the 508 it answers a request whose
//! `Via` already names it (clauses 5.6.6 and 6.3.18).

mod d018_01;
mod d018_02;
mod d018_03;
mod via_header;
