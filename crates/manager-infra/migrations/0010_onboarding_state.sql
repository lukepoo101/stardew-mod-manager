-- Migration 0010: a database that never registered a game has not finished setup.
--
-- Migration 0003 seeded app_context with 'completed' for every database, so an
-- upgraded installation would not be sent through setup again. It did the same
-- for a brand-new database, which left a fresh install looking finished: no
-- Skip, and a restart part way through setup landed on a dashboard that could
-- not launch anything. Finishing or skipping setup is what writes 'completed'
-- or 'skipped' now.
--
-- Only a 'completed' row with no game installation at all is reset. That is
-- every fresh database, and no one who really finished setup, because setup
-- cannot finish without a game. An upgraded database with a game keeps
-- 'completed' as before.
UPDATE app_context
SET onboarding_disposition = 'not_started',
    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE onboarding_disposition = 'completed'
  AND NOT EXISTS (SELECT 1 FROM game_installations);
