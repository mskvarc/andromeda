//! Delivery to clients that hold the connection open.
//!
//! SSE and WebSocket invert the usual direction: the client opens a stream and
//! the broker pushes into it, so there is no endpoint to connect to and
//! nothing to retry. What replaces the transport is a registry of live
//! connections keyed by subscription, and both protocols want exactly the same
//! one: [`registry::ConnectionRegistry`], parameterised by the payload each
//! sends.

pub mod registry;
pub mod sse;
pub mod ws;
