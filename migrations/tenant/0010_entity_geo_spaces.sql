-- Add observation_space and operation_space GeoProperty columns to entities
-- (ETSI GS CIM 009 clause 5.2.4, Table 5.2.4-1).
ALTER TABLE entities ADD COLUMN observation_space GEOMETRY(Geometry, 4326);
ALTER TABLE entities ADD COLUMN operation_space GEOMETRY(Geometry, 4326);
