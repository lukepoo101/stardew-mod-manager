-- Rust migration code first rewrites legacy IDs to the deterministic identity
-- and merges duplicate rows. This statement is the final database invariant.
CREATE UNIQUE INDEX IF NOT EXISTS uq_package_components_identity
ON package_components (artifact_hash, relative_component_root, unique_id);
