-- Clause 5.2.23 requires every stored Query object to carry type="Query".
-- Rows written by earlier broker versions omitted that discriminator. Rewrite
-- only missing members, preserving each JSON object and its array position.
UPDATE snapshots
SET snapshot_queries = (
    SELECT jsonb_agg(
        CASE
            WHEN element ? 'type' THEN element
            ELSE jsonb_set(element, '{type}', '"Query"'::jsonb, true)
        END
        ORDER BY ordinality
    )
    FROM jsonb_array_elements(snapshot_queries) WITH ORDINALITY AS query(element, ordinality)
)
WHERE snapshot_queries IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM jsonb_array_elements(snapshot_queries) AS query(element)
      WHERE NOT element ? 'type'
  );

UPDATE snapshots
SET snapshot_temporal_queries = (
    SELECT jsonb_agg(
        CASE
            WHEN element ? 'type' THEN element
            ELSE jsonb_set(element, '{type}', '"Query"'::jsonb, true)
        END
        ORDER BY ordinality
    )
    FROM jsonb_array_elements(snapshot_temporal_queries) WITH ORDINALITY AS query(element, ordinality)
)
WHERE snapshot_temporal_queries IS NOT NULL
  AND EXISTS (
      SELECT 1
      FROM jsonb_array_elements(snapshot_temporal_queries) AS query(element)
      WHERE NOT element ? 'type'
  );
