//! The federation of the paged-vehicle tests: a context source holding 2,500 Entities of one type, answering them in
//! pages of 1,000 linked by `next` (clause 6.3.10), beside the 14 Entities the broker holds itself, 4 of which
//! interleave the source's identifiers and 10 of which share them (clause 5.7.2.4).

use crate::{context_source::ContextSource, namespace::Namespace, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::model::{Entity, Input, Output, Property, evolution::TemporalEntity};

/// The Entities the source holds.
const SOURCE_ENTITIES: i64 = 2500;

/// The Entities in one page of the source.
const PAGE: i64 = 1000;

/// The identifier suffixes of the Entities the broker holds itself: four that interleave the source's identifiers
/// (they end in `L`), and ten that the source holds too.
const LOCAL_SUFFIXES: [&str; 14] = [
    "00000L", "00999L", "01000L", "01500L", "02490", "02491", "02492", "02493", "02494", "02495", "02496", "02497", "02498", "02499",
];

/// The Entities the source holds from number `first` to number `last`, both included, numbered from 00000.
fn source_entities(namespace: &Namespace, first: i64, last: i64) -> Result<Vec<Entity<Output>>> {
    let paged_vehicle_type = namespace.entity_type("PagedVehicle")?;
    (first..=last)
        .map(|number| -> Result<Entity<Output>> {
            Ok(Entity::output_builder()
                .id(namespace.urn("PagedVehicle", format!("{number:05}"))?)
                .entity_type(paged_vehicle_type.clone())
                .property("remote", number)
                .build()?)
        })
        .collect()
}

/// Makes `source` answer Query Entities under the prefix `a` with its 2,500 Entities in pages of 1,000, each page but
/// the last linking the next by an offset.
///
/// # Errors
///
/// Returns the failure of building an Entity, of writing the link or of mounting an answer.
pub async fn serve_pages(source: &ContextSource, namespace: &Namespace) -> Result<()> {
    let mut first = 0;
    while first < SOURCE_ENTITIES {
        let last = (first + PAGE - 1).min(SOURCE_ENTITIES - 1);
        let entities = source_entities(namespace, first, last)?;
        let answer = source_answer::entities(&entities);
        let request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities");
        if last + 1 < SOURCE_ENTITIES {
            let next = last + 1;
            let linked = source_answer::linked_to_next(
                answer,
                format!("/a/ngsi-ld/v1/entities?type=PagedVehicle_{namespace}&limit={PAGE}&offset={next}").parse()?,
            )?;
            let request = if first == 0 {
                request.priority(5)
            } else {
                request.query("offset", first).priority(1)
            };
            source.answer_when(request, linked).await?;
        } else {
            source.answer_when(request.query("offset", first).priority(1), answer).await?;
        }
        first += PAGE;
    }
    Ok(())
}

/// The 14 Entities the broker holds itself.
///
/// # Errors
///
/// Returns the failure of building an identifier or an Entity.
pub fn local_entities(namespace: &Namespace) -> Result<Vec<Entity<Input>>> {
    let paged_vehicle_type = namespace.entity_type("PagedVehicle")?;
    LOCAL_SUFFIXES
        .iter()
        .map(|suffix| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("PagedVehicle", suffix)?)
                .entity_type(paged_vehicle_type.clone())
                .property("local", true)
                .build()?)
        })
        .collect()
}

/// The evolutions the source holds from number `first` to number `last`, both included, numbered from 00000.
fn source_evolutions(namespace: &Namespace, first: i64, last: i64) -> Result<Vec<TemporalEntity<Input>>> {
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    (first..=last)
        .map(|number| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("PagedTemporal", format!("{number:05}"))?)
                .entity_type(paged_temporal_type.clone())
                .property("remote", Property::builder(number).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect()
}

/// Makes `source` answer Query Temporal Evolution of Entities under the prefix `a` with its 2,500 evolutions in pages
/// of 1,000, each page but the last linking the next by an offset.
///
/// # Errors
///
/// Returns the failure of building an evolution, of writing the link or of mounting an answer.
pub async fn serve_temporal_pages(source: &ContextSource, namespace: &Namespace) -> Result<()> {
    let mut first = 0;
    while first < SOURCE_ENTITIES {
        let last = (first + PAGE - 1).min(SOURCE_ENTITIES - 1);
        let answer = source_answer::temporal_entities(&source_evolutions(namespace, first, last)?);
        let request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities");
        let request = if first == 0 {
            request.priority(5)
        } else {
            request.query("offset", first).priority(1)
        };
        if last + 1 < SOURCE_ENTITIES {
            let next = last + 1;
            let linked = source_answer::linked_to_next(
                answer,
                format!("/a/ngsi-ld/v1/temporal/entities?type=PagedTemporal_{namespace}&limit={PAGE}&offset={next}").parse()?,
            )?;
            source.answer_when(request, linked).await?;
        } else {
            source.answer_when(request, answer).await?;
        }
        first += PAGE;
    }
    Ok(())
}

/// The 14 evolutions the broker holds itself, one instant each.
///
/// # Errors
///
/// Returns the failure of building an identifier or an evolution.
pub fn local_evolutions(namespace: &Namespace) -> Result<Vec<TemporalEntity<Input>>> {
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    LOCAL_SUFFIXES
        .iter()
        .map(|suffix| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("PagedTemporal", suffix)?)
                .entity_type(paged_temporal_type.clone())
                .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source::ContextSource,
        namespace::Namespace,
        paged_federation::{local_entities, serve_pages},
    };
    use googletest::prelude::*;

    #[gtest]
    fn the_broker_holds_fourteen_entities_of_the_namespace() -> Result<()> {
        let namespace = Namespace::fresh();
        let local = local_entities(&namespace)?;
        expect_that!(local.len(), eq(14));
        verify_that!(local.first().map(|entity| entity.id.to_string()), some(ends_with("-00000L")))
    }

    #[gtest]
    fn the_broker_holds_fourteen_evolutions_of_the_namespace() -> Result<()> {
        let namespace = Namespace::fresh();
        verify_that!(super::local_evolutions(&namespace)?.len(), eq(14))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_source_serves_its_three_pages() -> Result<()> {
        let source = ContextSource::start().await?;
        verify_that!(serve_pages(&source, &Namespace::fresh()).await.is_ok(), eq(true))
    }
}
