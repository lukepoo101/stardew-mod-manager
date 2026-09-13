CREATE TABLE IF NOT EXISTS preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO preferences (key, value, updated_at)
SELECT substr(step_kind, 6), payload_json, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM operation_steps
WHERE operation_id = '00000000-0000-0000-0000-000000000000'
  AND step_kind LIKE 'pref_%';

DELETE FROM operation_steps
WHERE operation_id = '00000000-0000-0000-0000-000000000000'
  AND step_kind LIKE 'pref_%';
