-- Migration 0001: Initial schema
CREATE TABLE game_installations (
    id TEXT PRIMARY KEY,
    canonical_root TEXT NOT NULL,
    platform_kind TEXT NOT NULL,
    detected_version TEXT,
    validated_at TEXT NOT NULL,
    is_fresh INTEGER NOT NULL,
    validation_error TEXT
);

CREATE TABLE setups (
    id TEXT PRIMARY KEY,
    game_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    relative_mods_dir TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (game_id) REFERENCES game_installations(id) ON DELETE CASCADE
);

CREATE TABLE packages (
    hash TEXT PRIMARY KEY,
    original_filename TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE installed_mods (
    id TEXT PRIMARY KEY,
    setup_id TEXT NOT NULL,
    package_id TEXT NOT NULL,
    unique_id TEXT NOT NULL,
    name TEXT NOT NULL,
    author TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    raw_manifest TEXT NOT NULL,
    relative_target_path TEXT NOT NULL,
    file_inventory_json TEXT NOT NULL,
    installed_at TEXT NOT NULL,
    FOREIGN KEY (setup_id) REFERENCES setups(id) ON DELETE CASCADE
);

CREATE TABLE operations (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    error_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL
);

CREATE TABLE launch_sessions (
    id TEXT PRIMARY KEY,
    game_id TEXT NOT NULL,
    setup_id TEXT NOT NULL,
    launched_at TEXT NOT NULL,
    pid INTEGER,
    state TEXT NOT NULL,
    expected_mod_ids_json TEXT NOT NULL,
    log_baseline_time TEXT,
    verification_result_json TEXT
);

CREATE TABLE smapi_installations (
    id TEXT PRIMARY KEY,
    game_id TEXT NOT NULL UNIQUE,
    release_version TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    observed_version TEXT,
    installed_at TEXT NOT NULL,
    FOREIGN KEY (game_id) REFERENCES game_installations(id) ON DELETE CASCADE
);

CREATE TABLE window_geometry (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    x INTEGER NOT NULL,
    y INTEGER NOT NULL,
    is_maximized INTEGER NOT NULL
);
