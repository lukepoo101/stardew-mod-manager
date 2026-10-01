-- Migration 0009: what a session started on, and what the user accepted.
--
-- runtime_json holds the game and SMAPI versions observed just before the
-- process started, so a session can later be compared with the runtime it
-- actually ran on. acknowledged_warnings_json holds the non-blocking preflight
-- warnings the user reviewed and launched past. Rows written before this
-- migration have neither and read back as unknown and none.
ALTER TABLE launch_sessions ADD COLUMN runtime_json TEXT;
ALTER TABLE launch_sessions ADD COLUMN acknowledged_warnings_json TEXT;
