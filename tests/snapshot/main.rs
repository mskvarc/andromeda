//! Snapshot conformance and reliability acceptance suite.

#![allow(clippy::expect_used, reason = "integration test transport failures must stop the case")]

mod capture;
mod core;
mod coverage;
mod lifecycle;
mod model_protocol;
#[path = "../support/notification_adapter.rs"]
mod notification_adapter;
mod purge;
mod stress;
mod subscriptions;
mod support;
mod temporal;
mod tenancy_resilience;

use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;

pub(crate) use support::*;
