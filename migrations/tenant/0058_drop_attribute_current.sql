-- ============================================================================
-- 0058: Drop the superseded current-attribute projection
-- ============================================================================
-- `current_attribute_values` is loaded and validated, and nothing references
-- `attribute_current` any more: its foreign key onto the temporal catalogue went
-- in 0054, its foreign key onto `entities(id)` in 0056, and the unit-code
-- foreign key it still owns points outward and disappears with it. No view, no
-- trigger and no function reads it -- 0054 left this schema owning exactly one
-- function, `geom_to_geog`, and no triggers at all.
--
-- No `IF EXISTS`: the table is known to exist at this point, and a missing one
-- means the database is not in the state the preceding migrations left it in,
-- which must fail rather than be tolerated.
--
-- No `CASCADE` either. If some object this migration set does not know about has
-- come to depend on the table, the right outcome is a failed migration naming
-- it, not a silent drop of whatever hung off it.
-- ============================================================================

DROP TABLE attribute_current;
