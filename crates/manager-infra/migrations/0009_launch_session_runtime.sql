-- Migration 0009: record the game and SMAPI versions a session started with.
--
-- They are observed just before the process starts, so a session can later be
-- compared with the runtime it actually ran on. Rows written before this
-- migration have no observation and read back as unknown.
ALTER TABLE launch_sessions ADD COLUMN runtime_json TEXT;
