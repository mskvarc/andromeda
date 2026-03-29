use crate::{entity::OrderDirection, error::QueryError, q::Expr};

#[derive(Debug)]
pub struct CompiledEntityQuery {
    pub ids: Option<Vec<String>>,
    pub id_pattern: Option<String>,
    pub types: Option<Vec<String>>,
    pub attrs: Vec<String>,
    pub q_expr: Option<Expr>,
    pub geo: Option<GeoQuery>,
    pub limit: u32,
    pub offset: u32,
    pub count: bool,
    pub pick: Vec<String>,
    pub omit: Vec<String>,
    pub order_by: Vec<CompiledOrderTerm>,
    pub scope_q: Option<String>,
    pub type_match_all: bool,
}

#[derive(Debug)]
pub struct CompiledOrderTerm {
    pub attr_iri: String,
    pub direction: OrderDirection,
    /// ICU collation identifier for string sorting (§4.23).
    pub collation: Option<String>,
}

#[derive(Debug)]
pub struct GeoQuery {
    pub georel: GeoRel,
    pub geometry: String,
    pub coordinates: String,
    pub geoproperty_iri: String,
}

#[derive(Debug)]
pub enum GeoRel {
    Within,
    Contains,
    Intersects,
    Disjoint,
    Equals,
    Overlaps,
    Near { max_distance: Option<f64>, min_distance: Option<f64> },
}

impl GeoRel {
    pub fn parse(s: &str) -> Result<Self, QueryError> {
        let parts: Vec<&str> = s.split(';').collect();
        let rel = parts[0].trim();
        match rel {
            "within" => Ok(GeoRel::Within),
            "contains" => Ok(GeoRel::Contains),
            "intersects" => Ok(GeoRel::Intersects),
            "disjoint" => Ok(GeoRel::Disjoint),
            "equals" => Ok(GeoRel::Equals),
            "overlaps" => Ok(GeoRel::Overlaps),
            "near" => {
                let mut max_distance = None;
                let mut min_distance = None;
                for part in &parts[1..] {
                    let part = part.trim();
                    if let Some(val) = part.strip_prefix("maxDistance==") {
                        max_distance = Some(
                            val.parse::<f64>()
                                .map_err(|_| QueryError::BadRequestData("Invalid maxDistance value".to_string()))?,
                        );
                    } else if let Some(val) = part.strip_prefix("minDistance==") {
                        min_distance = Some(
                            val.parse::<f64>()
                                .map_err(|_| QueryError::BadRequestData("Invalid minDistance value".to_string()))?,
                        );
                    }
                }
                Ok(GeoRel::Near { max_distance, min_distance })
            }
            _ => Err(QueryError::BadRequestData(format!("Unknown geo relationship: {}", rel))),
        }
    }
}

#[derive(Debug)]
pub struct CompiledCsrQuery {
    pub ids: Option<Vec<String>>,
    pub entity_type: Option<String>,
    pub id_pattern: Option<String>,
    pub attrs: Option<Vec<String>>,
    pub q_expr: Option<Expr>,
    pub geo: Option<GeoQuery>,
    pub temporal: Option<CompiledCsrTemporalFilter>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug)]
pub struct CompiledCsrTemporalFilter {
    pub timerel: String,
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: Option<String>,
}
