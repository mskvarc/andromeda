//! A source that holds a long history and cuts its first answer: a `speed` instance a minute from 2020-01-01T00:00:00Z
//! (value = minutes since then), 250 of them, of which the source answers the first 100 with a 206 whose
//! `Content-Range` ends at the 100th, and every later request with the instances from the 100th on (clause 6.3.10).

use crate::{context_source::ContextSource, namespace::Namespace, source_request::SourceRequest};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_RANGE};
use ngsi_ld::{
    model::{DateTime, EntityId, EntityTypeName, Input, Lossless, Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};
use std::ops::RangeInclusive;
use wiremock::ResponseTemplate;

/// The path pattern of the temporal retrieval the source answers, beneath its root `a`.
const RETRIEVE_PATH: &str = "/a/ngsi-ld/v1/temporal/entities/[^/]+";

/// The part of the first request's `timeAt` that tells it from a resumed one.
const FIRST_REQUEST_TIME: &str = "2020-01-01T00:00:00";

/// The `Content-Range` the cut answer carries: it ends at the 100th instant of the stream.
const CUT_RANGE: &str = "DateTime 2020-01-01T00:00:00Z-2020-01-01T01:39:00Z/*";

/// The last minute of the cut answer, the 100th instance.
const CUT_LAST_MINUTE: u32 = 99;

/// The last instance of the stream.
const LAST_MINUTE: u32 = 249;

/// The instant `minutes` minutes after 2020-01-01T00:00:00Z.
///
/// # Errors
///
/// Returns the error of the instant when the text is not one, which a count of minutes of a few days never is.
pub fn instant(minutes: u32) -> Result<DateTime> {
    Ok(format!("2020-01-0{}T{:02}:{:02}:00Z", 1 + minutes / 1440, minutes % 1440 / 60, minutes % 60).parse()?)
}

/// The evolution of the entity `id` of `entity_type` holding the `speed` instances of `minutes`, each valued with its
/// minute and observed at it.
///
/// # Errors
///
/// Returns the error of an instant or of the build.
pub fn evolution(id: &EntityId, entity_type: &EntityTypeName, minutes: RangeInclusive<u32>) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(id.clone()).entity_type(entity_type.clone());
    for minute in minutes {
        builder = builder.property("speed", Property::builder(minute).observed_at(instant(minute)?));
    }
    Ok(builder.build()?)
}

/// The registration `registration_id` of `source` for the entity `id` of `entity_type`, with the four retrieval
/// operations the temporal and entity reads use and the default (inclusive) mode.
///
/// # Errors
///
/// Returns the error of the source's endpoint or of the build.
pub fn registration(source: &ContextSource, namespace: &Namespace, id: &EntityId, entity_type: &EntityTypeName) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(source.endpoint("a")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(entity_type.clone()).with_id(id.clone())])
            .build()?])
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?)
}

/// Stubs `source`: a request whose `timeAt` is the start of the stream is answered 206 with the first 100 instances
/// and the cut `Content-Range`; every other temporal retrieval is answered 200 with the instances from the 100th on.
///
/// # Errors
///
/// Returns the error of the build or of the source.
pub async fn serve(source: &ContextSource, id: &EntityId, entity_type: &EntityTypeName) -> Result<()> {
    let cut = evolution(id, entity_type, 0..=CUT_LAST_MINUTE)?;
    let rest = evolution(id, entity_type, CUT_LAST_MINUTE..=LAST_MINUTE)?;
    let cut_answer = ResponseTemplate::new(StatusCode::PARTIAL_CONTENT.as_u16())
        .set_body_raw(cut.render(Lossless::Normalized).to_string(), "application/json")
        .insert_header(CONTENT_RANGE, HeaderValue::from_static(CUT_RANGE));
    let rest_answer = ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_raw(rest.render(Lossless::Normalized).to_string(), "application/json");
    source
        .answer_when(
            SourceRequest::matching(Method::GET, RETRIEVE_PATH)
                .query_containing("timeAt", FIRST_REQUEST_TIME)
                .priority(1),
            cut_answer,
        )
        .await?;
    source
        .answer_when(SourceRequest::matching(Method::GET, RETRIEVE_PATH).priority(5), rest_answer)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        cut_stream::{evolution, instant},
        namespace::Namespace,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{DateTime, Lossless};

    #[gtest]
    fn the_last_minute_of_the_stream_is_four_hours_nine_minutes_in() -> Result<()> {
        expect_that!(instant(249)?, eq("2020-01-01T04:09:00Z".parse::<DateTime>()?));
        expect_that!(instant(0)?, eq("2020-01-01T00:00:00Z".parse::<DateTime>()?));
        Ok(())
    }

    #[gtest]
    fn a_range_of_minutes_holds_one_instance_each() -> Result<()> {
        let namespace = Namespace::fresh();
        let rendered = evolution(&namespace.urn("Vehicle", 1)?, &namespace.entity_type("Vehicle")?, 99..=249)?
            .render(Lossless::Normalized)
            .to_string();
        expect_that!(rendered.matches("observedAt").count(), eq(151));
        Ok(())
    }
}
