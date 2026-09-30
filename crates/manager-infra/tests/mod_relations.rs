//! The relations query explains why a mod is installed and traces unmet
//! requirements; the mod list reports real install times.

use chrono::{TimeZone, Utc};
use manager_app::ports::repositories::{
    DeploymentRepository, GameInstallationRepository, PackageCatalogRepository, ProfileRepository,
};
use manager_app::queries::ModsQueries;
use manager_core::deployment::{
    DeploymentState, InstalledReason, ProfileComponent, ProfileDeployment,
};
use manager_core::game::{GameInstallation, ManagementMode, OperatingSystem, Storefront};
use manager_core::ids::{
    ArtifactHash, DeploymentId, ModUniqueId, PackageComponentId, ProfileComponentId,
};
use manager_core::manifest::{Manifest, ModDependency};
use manager_core::package::{PackageArtifact, PackageComponent};
use manager_core::profile::Profile;
use manager_infra::db::SqliteStateRepository;
use std::sync::Arc;

fn manifest(id: &str, requires: &[(&str, bool)]) -> Manifest {
    Manifest {
        unique_id: ModUniqueId::new(id),
        name: id.split('.').next_back().unwrap().to_string(),
        author: "A".into(),
        version: "1.0.0".into(),
        description: None,
        entry_dll: None,
        minimum_api_version: None,
        minimum_game_version: None,
        update_keys: Vec::new(),
        dependencies: requires
            .iter()
            .map(|(dep, required)| ModDependency {
                unique_id: ModUniqueId::new(*dep),
                minimum_version: None,
                is_required: *required,
            })
            .collect(),
        content_pack_for: None,
    }
}

#[test]
fn relations_explain_reasons_and_trace_gaps() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Arc::new(SqliteStateRepository::new(tmp.path().join("s.sqlite3")).unwrap());
    let game = GameInstallation {
        id: manager_core::ids::GameInstallationId::new(),
        canonical_root: tmp.path().join("Game"),
        operating_system: OperatingSystem::Linux,
        storefront: Storefront::Steam,
        management_mode: ManagementMode::Managed,
        created_at: Utc::now(),
    };
    repo.save_game(&game).unwrap();
    let profile = Profile::new(game.id, "Main");
    repo.save_profile(&profile).unwrap();

    let installed = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
    let add = |index: usize, m: Manifest, reason: InstalledReason| -> ProfileComponentId {
        let hash = ArtifactHash::parse(format!("{index}").repeat(64)).unwrap();
        repo.save_artifact(&PackageArtifact {
            hash: hash.clone(),
            byte_size: 1,
            storage_relative_path: String::new(),
            first_seen_at: Utc::now(),
        })
        .unwrap();
        let deployment = ProfileDeployment {
            id: DeploymentId::new(),
            profile_id: profile.id,
            artifact_hash: hash.clone(),
            root_relative_path: m.name.clone(),
            installed_at: installed,
            state: DeploymentState::Present,
        };
        repo.save_deployment(&deployment).unwrap();
        let package_component_id = PackageComponentId::new();
        repo.save_package_component(&PackageComponent {
            id: package_component_id,
            artifact_hash: hash,
            unique_id: m.unique_id.clone(),
            name: m.name.clone(),
            author: m.author.clone(),
            version: m.version.clone(),
            description: None,
            relative_component_root: m.name.clone(),
            raw_manifest: "{}".into(),
            manifest: m,
        })
        .unwrap();
        let id = ProfileComponentId::new();
        repo.save_profile_component(&ProfileComponent {
            id,
            profile_id: profile.id,
            deployment_id: deployment.id,
            package_component_id,
            enabled: true,
            installed_reason: reason,
        })
        .unwrap();
        id
    };
    let top = add(
        1,
        manifest("A.Top", &[("a.framework", true), ("A.Nice", false)]),
        InstalledReason::Direct,
    );
    let framework = add(
        2,
        manifest("A.Framework", &[("A.Missing", true)]),
        InstalledReason::Dependency,
    );

    let queries = ModsQueries::new(repo.clone(), repo.clone());
    let top_rel = queries.get_mod_relations(&top).unwrap().unwrap();
    assert_eq!(top_rel.installed_reason, "direct");
    assert_eq!(
        top_rel.broken_chains,
        vec![vec![
            "Top".to_string(),
            "Framework".into(),
            "A.Missing".into()
        ]]
    );
    let statuses: Vec<_> = top_rel
        .requires
        .iter()
        .map(|r| (r.kind.as_str(), r.status.as_str()))
        .collect();
    assert_eq!(
        statuses,
        vec![("required", "satisfied"), ("optional", "missing")]
    );

    let framework_rel = queries.get_mod_relations(&framework).unwrap().unwrap();
    assert_eq!(framework_rel.installed_reason, "dependency");
    assert_eq!(
        framework_rel.reason_detail,
        "It was installed because Top needs it."
    );
    assert_eq!(framework_rel.required_by[0].name, "Top");

    let list = queries.list_profile_mods(&profile.id).unwrap();
    assert!(list
        .iter()
        .all(|m| m.installed_at == installed.to_rfc3339()));
}
