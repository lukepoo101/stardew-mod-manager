-- Canonical package components are unique within immutable artifact contents.
-- Collapse historical duplicates before adding the invariant.
CREATE TEMP TABLE package_component_identity_map AS
SELECT artifact_hash, relative_component_root, unique_id, MIN(id) AS canonical_id
FROM package_components
GROUP BY artifact_hash, relative_component_root, unique_id;

UPDATE profile_components
SET package_component_id = (
    SELECT canonical_id
    FROM package_component_identity_map m
    JOIN package_components c ON c.id = profile_components.package_component_id
    WHERE m.artifact_hash = c.artifact_hash
      AND m.relative_component_root = c.relative_component_root
      AND m.unique_id = c.unique_id
);

DELETE FROM package_components
WHERE id NOT IN (SELECT canonical_id FROM package_component_identity_map);

CREATE UNIQUE INDEX IF NOT EXISTS uq_package_components_identity
ON package_components (artifact_hash, relative_component_root, unique_id);

DROP TABLE package_component_identity_map;
