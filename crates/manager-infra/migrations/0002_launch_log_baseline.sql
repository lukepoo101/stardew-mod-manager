-- Migration 0002: Add log baseline JSON column to launch sessions
ALTER TABLE launch_sessions ADD COLUMN log_baseline_json TEXT;
