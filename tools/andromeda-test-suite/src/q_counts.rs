//! A table of queries and the number of entities each should select among one entity the test created, read in one
//! pass, so a test of the query language's spellings (clause 4.9) compares the whole table and a failure names the
//! query that answered otherwise.

use crate::{
    broker::BrokerClient,
    q_selection::{QSelectionError, selected_count},
};
use ngsi_ld::model::EntityId;

/// The queries of `table` with the number of entities each selects for the entity `id`, in the order of the table.
///
/// # Errors
///
/// Returns [`QSelectionError`] when a query is not of the grammar or the broker does not answer it with a page.
pub async fn observed<'q>(broker: &BrokerClient, id: &EntityId, table: &[(&'q str, usize)]) -> Result<Vec<(&'q str, usize)>, QSelectionError> {
    let mut counts = Vec::with_capacity(table.len());
    for (q, _) in table {
        counts.push((*q, selected_count(broker, id, q).await?));
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, namespace::Namespace, q_counts::observed};
    use googletest::prelude::*;
    use url::Url;
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
        matchers::{method, query_param},
    };

    #[gtest]
    #[tokio::test]
    async fn each_query_is_paired_with_the_count_it_selected() -> Result<()> {
        let namespace = Namespace::fresh();
        let id = namespace.urn("Observation", 1)?;
        let server = MockServer::start().await;
        let entity = format!(r#"[{{"id": "{id}", "type": "Observation"}}]"#);
        Mock::given(method("GET"))
            .and(query_param("q", "reading==1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string(entity),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(query_param("q", "reading==2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_string("[]"),
            )
            .mount(&server)
            .await;
        let client = client_at(Url::parse(&format!("{}/ngsi-ld/v1/", server.uri()))?)?;
        let counts = observed(&client, &id, &[("reading==1", 1), ("reading==2", 1)]).await?;
        verify_that!(counts, eq(&vec![("reading==1", 1), ("reading==2", 0)]))
    }
}
