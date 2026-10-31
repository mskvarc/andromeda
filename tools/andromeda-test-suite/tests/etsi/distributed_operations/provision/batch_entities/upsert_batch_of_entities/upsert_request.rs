//! The Batch Entity Upsert the upsert cases send: the entities posted to `/entityOperations/upsert` with the
//! `options` value written out, `replace` included, which the typed operation leaves out as the default.

use andromeda_test_suite::{broker::BrokerClient, problem::answered, wire::send_to};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::BatchUpsert,
    model::{Entity, Input},
};

/// `Batch Upsert Entities` of `entities` with `options=<options>` through `writer`: requires 204.
pub(crate) async fn upsert(writer: &BrokerClient, entities: &[Entity<Input>], options: &str) -> Result<()> {
    let operation = BatchUpsert::builder().entities(entities).build();
    let answer = send_to(writer, &operation, &format!("entityOperations/upsert?options={options}")).await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))
}
