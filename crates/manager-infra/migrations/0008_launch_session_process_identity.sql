-- Migration 0008: persist the launched process identity with its session.
--
-- A pid is recycled, so "pid 4242 is alive" after the manager restarts does not
-- prove that pid 4242 is still the process this session started. Recording the
-- kernel creation time and the image path alongside the pid is what makes the
-- question answerable. Rows written before this migration have no identity and
-- keep their pid-only behaviour.
ALTER TABLE launch_sessions ADD COLUMN process_identity_json TEXT;
