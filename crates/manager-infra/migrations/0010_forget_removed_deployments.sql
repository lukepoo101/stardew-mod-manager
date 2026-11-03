-- Migration 0010: forget the deployments of mods that were removed.
--
-- A committed removal used to leave its deployment row behind as
-- 'quarantined', so the removed mod's package went on counting as used by
-- the profile and storage cleanup never offered it. Removals now delete the
-- row in the same transaction. Rows left by earlier removals have no profile
-- components; recovery treats a missing row as a committed removal, exactly
-- as it treats a quarantined one.
DELETE FROM profile_deployments
WHERE state = 'quarantined'
  AND NOT EXISTS (
      SELECT 1 FROM profile_components pc
      WHERE pc.deployment_id = profile_deployments.id
  );
