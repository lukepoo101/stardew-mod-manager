-- Migration 0003: Architecture foundation (Profiles, Content-Addressed Packages, Deployments, Durable Operations, Health)

-- 1. Game installations enhancements
ALTER TABLE game_installations ADD COLUMN operating_system TEXT NOT NULL DEFAULT 'linux';
ALTER TABLE game_installations ADD COLUMN storefront TEXT NOT NULL DEFAULT 'steam';
ALTER TABLE game_installations ADD COLUMN management_mode TEXT NOT NULL DEFAULT 'managed';
ALTER TABLE game_installations ADD COLUMN created_at TEXT;
UPDATE game_installations SET created_at = validated_at WHERE created_at IS NULL;

-- 2. First-class profiles table
CREATE TABLE profiles (
    id TEXT PRIMARY KEY,
    game_installation_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    revision INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    FOREIGN KEY (game_installation_id) REFERENCES game_installations(id) ON DELETE CASCADE
);

-- Migrate existing setups to profiles
INSERT INTO profiles (id, game_installation_id, name, description, revision, created_at, updated_at, state)
SELECT id, game_id, display_name, NULL, 1, created_at, created_at, 'active' FROM setups;

-- 3. Packages enhancements
ALTER TABLE packages ADD COLUMN storage_relative_path TEXT NOT NULL DEFAULT '';
ALTER TABLE packages ADD COLUMN first_seen_at TEXT;
UPDATE packages SET first_seen_at = created_at WHERE first_seen_at IS NULL;
UPDATE packages SET storage_relative_path = 'packages/' || hash || '.zip' WHERE storage_relative_path = '';

-- 4. Package acquisitions
CREATE TABLE acquisitions (
    id TEXT PRIMARY KEY,
    artifact_hash TEXT NOT NULL,
    source TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    expected_hash TEXT,
    source_metadata TEXT,
    FOREIGN KEY (artifact_hash) REFERENCES packages(hash) ON DELETE CASCADE
);

INSERT OR IGNORE INTO acquisitions (id, artifact_hash, source, original_filename, acquired_at)
SELECT 'acq-' || hash, hash, CASE source_kind WHEN 'direct_download' THEN 'direct_url' WHEN 'internal_pinned' THEN 'provider' ELSE 'local_file' END, original_filename, created_at FROM packages;

-- 5. Package components (manifests inside packages)
CREATE TABLE package_components (
    id TEXT PRIMARY KEY,
    artifact_hash TEXT NOT NULL,
    unique_id TEXT NOT NULL,
    name TEXT NOT NULL,
    author TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL,
    description TEXT,
    relative_component_root TEXT NOT NULL,
    raw_manifest TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    FOREIGN KEY (artifact_hash) REFERENCES packages(hash) ON DELETE CASCADE
);

INSERT OR IGNORE INTO package_components (id, artifact_hash, unique_id, name, author, version, description, relative_component_root, raw_manifest, manifest_json)
SELECT 'pc-' || id, package_id, unique_id, name, author, version, description, '', raw_manifest, raw_manifest FROM installed_mods;

-- 6. Profile deployments
CREATE TABLE profile_deployments (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    root_relative_path TEXT NOT NULL,
    installed_at TEXT NOT NULL,
    state TEXT NOT NULL,
    FOREIGN KEY (profile_id) REFERENCES profiles(id) ON DELETE CASCADE,
    FOREIGN KEY (artifact_hash) REFERENCES packages(hash) ON DELETE CASCADE
);

INSERT OR IGNORE INTO profile_deployments (id, profile_id, artifact_hash, root_relative_path, installed_at, state)
SELECT 'dep-' || id, setup_id, package_id, relative_target_path, installed_at, 'present' FROM installed_mods;

-- 7. Profile components
CREATE TABLE profile_components (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL,
    deployment_id TEXT NOT NULL,
    package_component_id TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    installed_reason TEXT NOT NULL DEFAULT 'direct',
    FOREIGN KEY (profile_id) REFERENCES profiles(id) ON DELETE CASCADE,
    FOREIGN KEY (deployment_id) REFERENCES profile_deployments(id) ON DELETE CASCADE,
    FOREIGN KEY (package_component_id) REFERENCES package_components(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO profile_components (id, profile_id, deployment_id, package_component_id, enabled, installed_reason)
SELECT id, setup_id, 'dep-' || id, 'pc-' || id, 1, 'direct' FROM installed_mods;

-- 8. SMAPI installations enhancements
ALTER TABLE smapi_installations ADD COLUMN release_policy_id TEXT NOT NULL DEFAULT 'default';

-- 9. Launch sessions enhancements
ALTER TABLE launch_sessions ADD COLUMN launch_mode TEXT NOT NULL DEFAULT 'modded';
ALTER TABLE launch_sessions ADD COLUMN ended_at TEXT;

-- 10. Operations schema extensions
ALTER TABLE operations ADD COLUMN game_installation_id TEXT;
ALTER TABLE operations ADD COLUMN profile_id TEXT;
ALTER TABLE operations ADD COLUMN expected_profile_revision INTEGER;
ALTER TABLE operations ADD COLUMN plan_schema_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE operations ADD COLUMN progress_current INTEGER;
ALTER TABLE operations ADD COLUMN progress_total INTEGER;
ALTER TABLE operations ADD COLUMN error_code TEXT;
ALTER TABLE operations ADD COLUMN cancellation_requested INTEGER NOT NULL DEFAULT 0;
ALTER TABLE operations ADD COLUMN completed_at TEXT;
ALTER TABLE operations ADD COLUMN rollback_plan_json TEXT;

CREATE TABLE operation_resources (
    operation_id TEXT NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    access_mode TEXT NOT NULL,
    PRIMARY KEY (operation_id, resource_kind, resource_id),
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

CREATE TABLE operation_steps (
    operation_id TEXT NOT NULL,
    step_index INTEGER NOT NULL,
    step_kind TEXT NOT NULL,
    state TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    error_json TEXT,
    PRIMARY KEY (operation_id, step_index),
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

CREATE TABLE operation_effects (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    profile_id TEXT,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    change_kind TEXT NOT NULL,
    before_json TEXT,
    after_json TEXT,
    occurred_at TEXT NOT NULL,
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

-- 10. App context and game profile context
CREATE TABLE app_context (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    active_game_installation_id TEXT,
    onboarding_disposition TEXT NOT NULL DEFAULT 'not_started',
    updated_at TEXT NOT NULL
);

INSERT OR IGNORE INTO app_context (id, active_game_installation_id, onboarding_disposition, updated_at)
VALUES (
    1,
    (SELECT id FROM game_installations LIMIT 1),
    'completed',
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
);

CREATE TABLE game_profile_context (
    game_installation_id TEXT PRIMARY KEY,
    active_profile_id TEXT,
    default_profile_id TEXT,
    last_active_profile_id TEXT,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (game_installation_id) REFERENCES game_installations(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO game_profile_context (game_installation_id, active_profile_id, default_profile_id, last_active_profile_id, updated_at)
SELECT game_id, id, id, id, strftime('%Y-%m-%dT%H:%M:%SZ', 'now') FROM setups;

-- 11. Health findings
CREATE TABLE findings (
    id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL UNIQUE,
    profile_id TEXT,
    code TEXT NOT NULL,
    severity TEXT NOT NULL,
    category TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    technical_details TEXT,
    evidence_json TEXT NOT NULL,
    action_json TEXT,
    created_at TEXT NOT NULL
);
