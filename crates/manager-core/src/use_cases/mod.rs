use crate::domain::*;
use crate::install::{InstallPlan, RemovalPlan};
use crate::launch::LaunchSpec;
use crate::ports::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub selected_game: Option<GameInstallation>,
    pub setup: Option<Setup>,
    pub smapi_installed: bool,
    pub smapi_version: Option<String>,
    pub installed_mods: Vec<InstalledMod>,
    pub active_operation: Option<Operation>,
    pub active_session: Option<LaunchSession>,
    pub recovery_error: Option<String>,
}

pub struct CoreUseCases<R, P, S, L, Log, Lock>
where
    R: StateRepository,
    P: PackageStore,
    S: SmapiInstaller,
    L: GameLauncher,
    Log: SessionLogReader,
    Lock: InstanceLock,
{
    pub repo: R,
    pub package_store: P,
    pub smapi_installer: S,
    pub launcher: L,
    pub log_reader: Log,
    pub lock: Lock,
}

impl<R, P, S, L, Log, Lock> CoreUseCases<R, P, S, L, Log, Lock>
where
    R: StateRepository,
    P: PackageStore,
    S: SmapiInstaller,
    L: GameLauncher,
    Log: SessionLogReader,
    Lock: InstanceLock,
{
    pub fn new(
        repo: R,
        package_store: P,
        smapi_installer: S,
        launcher: L,
        log_reader: Log,
        lock: Lock,
    ) -> Self {
        Self {
            repo,
            package_store,
            smapi_installer,
            launcher,
            log_reader,
            lock,
        }
    }

    fn ensure_no_pending_operations(&self) -> Result<(), String> {
        if !self.repo.list_unresolved_operations()?.is_empty() {
            return Err(
                "Recovery required: retry recovery before changing mods or launching the game"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn get_app_snapshot(&self, game_id: Option<&str>) -> Result<AppSnapshot, String> {
        let game = match game_id {
            Some(id) => self.repo.get_game(id)?,
            None => self.repo.list_games()?.into_iter().next(),
        };

        let (setup, smapi_rec, mods) = if let Some(ref g) = game {
            let s = self.repo.get_default_setup(&g.id)?;
            let smapi = self.repo.get_smapi_installation(&g.id)?;
            let m = if let Some(ref set) = s {
                self.repo.list_installed_mods(&set.id)?
            } else {
                Vec::new()
            };
            (s, smapi, m)
        } else {
            (None, None, Vec::new())
        };

        let active_op = self.repo.list_unresolved_operations()?.into_iter().next();

        let active_session = if let Some(ref g) = game {
            if let Ok(Some(latest_sess)) = self.repo.get_latest_launch_session(Some(&g.id)) {
                self.poll_session(&latest_sess.id).ok().flatten()
            } else {
                None
            }
        } else {
            None
        };

        Ok(AppSnapshot {
            selected_game: game,
            setup,
            smapi_installed: smapi_rec.is_some(),
            smapi_version: smapi_rec.map(|s| s.release_version),
            installed_mods: mods,
            active_operation: active_op,
            active_session,
            recovery_error: None,
        })
    }

    pub fn inspect_game(
        &self,
        path: &Path,
        platform_kind: StoreKind,
    ) -> Result<GameInstallation, String> {
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize path '{}': {}", path.display(), e))?;

        if let Ok(existing_games) = self.repo.list_games() {
            if let Some(mut existing) = existing_games
                .into_iter()
                .find(|g| g.canonical_root == canonical)
            {
                let managed_val =
                    crate::game::validate_managed_game(&canonical, existing.platform_kind);
                if managed_val.is_valid {
                    existing.is_managed = true;
                    existing.validation_error = None;
                    existing.detected_version = managed_val.detected_version;
                }
                return Ok(existing);
            }
        }

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(canonical.to_string_lossy().as_bytes());
        let path_hash = format!("{:x}", hasher.finalize());
        let id = format!("game-{}", &path_hash[..16]);
        let game = crate::game::create_game_installation(&id, canonical, platform_kind);
        Ok(game)
    }

    pub fn accept_game(&self, game: &GameInstallation) -> Result<GameInstallation, String> {
        if !game.is_fresh && !game.is_managed {
            return Err("Game candidate has not passed validation".into());
        }
        self.repo.save_game(game)?;

        if (game.is_fresh || game.is_managed) && self.repo.get_default_setup(&game.id)?.is_none() {
            let setup = Setup {
                id: format!("setup-{}", uuid_v4()),
                game_id: game.id.clone(),
                display_name: "Default".to_string(),
                relative_mods_dir: format!("setups/{}/Mods", uuid_v4()),
                created_at: Utc::now(),
            };
            self.repo.save_setup(&setup)?;
        }

        Ok(game.clone())
    }

    pub fn install_smapi(
        &self,
        game_id: &str,
        installer_archive: Option<&Path>,
    ) -> Result<SmapiInstallationRecord, String> {
        let _guard = self.lock.acquire_guard()?;
        self.ensure_no_pending_operations()?;

        let game = self
            .repo
            .get_game(game_id)?
            .ok_or_else(|| format!("Game '{}' not found", game_id))?;

        if !game.is_fresh {
            return Err("Cannot install SMAPI: Game installation is not fresh".to_string());
        }

        if self.launcher.is_game_running(None) {
            return Err("Cannot install SMAPI: Stardew Valley is currently running".to_string());
        }

        let op_id = format!("op-{}", uuid_v4());
        let op = Operation {
            id: op_id.clone(),
            kind: OperationKind::SmapiSetup,
            state: OperationState::Running,
            plan_json: serde_json::json!({
                "game_id": game_id,
                "version": crate::smapi::PINNED_SMAPI_VERSION,
            })
            .to_string(),
            error_json: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            schema_version: 1,
        };
        self.repo.save_operation(&op)?;

        match self
            .smapi_installer
            .install_smapi(&game.canonical_root, installer_archive)
        {
            Ok(mut rec) => {
                rec.game_id = game.id.clone();
                self.repo.save_smapi_installation(&rec)?;
                self.repo
                    .update_operation_state(&op_id, OperationState::Completed, None)?;
                Ok(rec)
            }
            Err(e) => {
                self.repo.update_operation_state(
                    &op_id,
                    OperationState::Failed,
                    Some(e.clone()),
                )?;
                Err(format!("SMAPI installation failed: {}", e))
            }
        }
    }

    pub fn commit_mod_install(
        &self,
        plan: &InstallPlan,
        staging_dir: &Path,
        final_mods_dir: &Path,
    ) -> Result<InstalledMod, String> {
        let _guard = self.lock.acquire_guard()?;

        if self.launcher.is_game_running(None) {
            return Err("Cannot install mod: Stardew Valley is currently running".to_string());
        }

        // Revalidate destination does not exist
        let dest_folder = final_mods_dir.join(&plan.mod_folder_name);
        if dest_folder.exists() {
            return Err(format!(
                "Destination folder '{}' already exists",
                dest_folder.display()
            ));
        }

        crate::install::validate_relative_path(&plan.mod_folder_name)?;
        self.ensure_no_pending_operations()?;

        // Revalidate dependencies
        let installed: Vec<(crate::ids::ModUniqueId, String)> = self
            .repo
            .list_installed_mods(&plan.setup_id)?
            .into_iter()
            .map(|m| (crate::ids::ModUniqueId::new(m.unique_id), m.version))
            .collect();
        let manifests = if plan.component_manifests.is_empty() {
            vec![plan.manifest.clone()]
        } else {
            plan.component_manifests
                .iter()
                .map(|c| c.manifest.clone())
                .collect()
        };
        let dep_report = crate::dependency::evaluate_bundle_dependencies(
            &manifests,
            &installed,
            Some(crate::smapi::PINNED_SMAPI_VERSION),
        );
        if !dep_report.is_installable {
            return Err("Cannot install mod: Dependencies are no longer satisfied".to_string());
        }

        let op_id = format!("op-{}", uuid_v4());
        let op = Operation {
            id: op_id.clone(),
            kind: OperationKind::ModInstall,
            state: OperationState::Prepared,
            plan_json: serde_json::to_string(plan).unwrap_or_default(),
            error_json: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            schema_version: 1,
        };
        self.repo.save_operation(&op)?;

        // Filesystem operation: Move from staging to final Mods folder
        let staged_mod_source = if staging_dir
            .join(&plan.plan_id)
            .join(&plan.mod_folder_name)
            .exists()
        {
            staging_dir.join(&plan.plan_id).join(&plan.mod_folder_name)
        } else {
            staging_dir.join(&plan.mod_folder_name)
        };
        if !staged_mod_source.exists() {
            self.repo.update_operation_state(
                &op_id,
                OperationState::Failed,
                Some("Staged mod source missing".to_string()),
            )?;
            return Err(format!(
                "Staged mod folder '{}' does not exist",
                staged_mod_source.display()
            ));
        }

        // Commit-time integrity revalidation of staged content
        if !plan.trusted_inventory.is_empty() {
            if let Err(e) = plan.verify_staged_content(&staged_mod_source) {
                let err_msg = format!("Staged mod integrity verification failed: {}", e);
                let _ = self.repo.update_operation_state(
                    &op_id,
                    OperationState::Failed,
                    Some(err_msg.clone()),
                );
                return Err(err_msg);
            }
        }

        if let Some(parent) = dest_folder.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create destination parent folder: {}", e))?;
        }

        std::fs::rename(&staged_mod_source, &dest_folder).map_err(|e| {
            let err_msg = format!("Failed to move staged mod into final destination: {}", e);
            let _ = self.repo.update_operation_state(
                &op_id,
                OperationState::Failed,
                Some(err_msg.clone()),
            );
            err_msg
        })?;

        // Database commit in ONE SQLite transaction
        let (all_items, primary_item) = if plan.component_manifests.is_empty() {
            let mod_item = InstalledMod {
                id: format!("mod-{}", uuid_v4()),
                setup_id: plan.setup_id.clone(),
                package_id: plan.package_hash.clone(),
                unique_id: plan.manifest.unique_id.to_string(),
                name: plan.manifest.name.clone(),
                author: plan.manifest.author.clone(),
                version: plan.manifest.version.clone(),
                description: plan.manifest.description.clone(),
                raw_manifest: plan.raw_manifest.clone(),
                relative_target_path: plan.mod_folder_name.clone(),
                file_inventory: plan.file_inventory.clone(),
                installed_at: Utc::now(),
            };
            (vec![mod_item.clone()], mod_item)
        } else {
            let mut items = Vec::new();
            let mut primary: Option<InstalledMod> = None;
            for comp in &plan.component_manifests {
                let rel_path = if comp.relative_subfolder.is_empty() {
                    plan.mod_folder_name.clone()
                } else {
                    format!("{}/{}", plan.mod_folder_name, comp.relative_subfolder)
                };

                let item = InstalledMod {
                    id: format!("mod-{}", uuid_v4()),
                    setup_id: plan.setup_id.clone(),
                    package_id: plan.package_hash.clone(),
                    unique_id: comp.manifest.unique_id.to_string(),
                    name: comp.manifest.name.clone(),
                    author: comp.manifest.author.clone(),
                    version: comp.manifest.version.clone(),
                    description: comp.manifest.description.clone(),
                    raw_manifest: comp.raw_manifest.clone(),
                    relative_target_path: rel_path,
                    file_inventory: plan.file_inventory.clone(),
                    installed_at: Utc::now(),
                };

                if primary.is_none() || comp.manifest.unique_id == plan.manifest.unique_id {
                    primary = Some(item.clone());
                }
                items.push(item);
            }
            let p = primary.unwrap_or_else(|| items[0].clone());
            (items, p)
        };

        if let Err(e) = self
            .repo
            .commit_bundle_install_transaction(&op_id, &all_items)
        {
            let err_msg = format!("Failed to commit install database transaction: {}", e);
            let _ = self.repo.update_operation_state(
                &op_id,
                OperationState::Recovering,
                Some(err_msg.clone()),
            );
            return Err(err_msg);
        }

        // Clean up plan staging directory on success
        let plan_staging_root = staging_dir.join(&plan.plan_id);
        if plan_staging_root.exists() {
            let _ = std::fs::remove_dir_all(&plan_staging_root);
        }

        Ok(primary_item)
    }

    pub fn remove_mod(
        &self,
        installed_mod_id: &str,
        setup_id: &str,
        mods_dir: &Path,
        recovery_dir: &Path,
    ) -> Result<(), String> {
        let _guard = self.lock.acquire_guard()?;
        self.ensure_no_pending_operations()?;

        if self.launcher.is_game_running(None) {
            return Err("Cannot remove mod: Stardew Valley is currently running".to_string());
        }

        let installed_mod = self
            .repo
            .get_installed_mod(installed_mod_id)?
            .ok_or_else(|| format!("Mod '{}' not found", installed_mod_id))?;

        let same_package: Vec<InstalledMod> = self
            .repo
            .list_installed_mods(setup_id)?
            .into_iter()
            .filter(|m| m.package_id == installed_mod.package_id)
            .collect();

        let bundle_mod_ids: Vec<String> = same_package.iter().map(|m| m.id.clone()).collect();
        let root_folder = Path::new(&installed_mod.relative_target_path)
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_else(|| installed_mod.relative_target_path.clone());

        let bundle_root_folder = if same_package.len() > 1 {
            Some(root_folder.clone())
        } else {
            None
        };

        let op_id = format!("op-{}", uuid_v4());
        let target_folder_to_remove = if same_package.len() > 1 {
            root_folder
        } else {
            installed_mod.relative_target_path.clone()
        };

        let recovery_target = recovery_dir.join(&op_id).join(&target_folder_to_remove);

        let removal_plan = RemovalPlan {
            operation_id: op_id.clone(),
            setup_id: setup_id.to_string(),
            installed_mod_id: installed_mod_id.to_string(),
            mod_unique_id: installed_mod.unique_id.clone(),
            relative_folder_path: target_folder_to_remove.clone(),
            recovery_folder_path: recovery_target.to_string_lossy().to_string(),
            bundle_mod_ids: bundle_mod_ids.clone(),
            bundle_root_folder,
        };

        // Persist removal plan before moving files
        let op = Operation {
            id: op_id.clone(),
            kind: OperationKind::ModRemove,
            state: OperationState::Prepared,
            plan_json: serde_json::to_string(&removal_plan).unwrap_or_default(),
            error_json: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            schema_version: 1,
        };
        self.repo.save_operation(&op)?;

        let source_path = mods_dir.join(&target_folder_to_remove);
        if source_path.exists() {
            if let Some(p) = recovery_target.parent() {
                std::fs::create_dir_all(p)
                    .map_err(|e| format!("Failed to create recovery parent directory: {}", e))?;
            }
            std::fs::rename(&source_path, &recovery_target).map_err(|e| {
                let err = format!("Failed to move mod to recovery directory: {}", e);
                let _ = self.repo.update_operation_state(
                    &op_id,
                    OperationState::Failed,
                    Some(err.clone()),
                );
                err
            })?;
        }

        // Commit removal in ONE SQLite transaction!
        if let Err(e) = self
            .repo
            .commit_bundle_remove_transaction(&op_id, &bundle_mod_ids)
        {
            let err = format!("Failed to commit removal database transaction: {}", e);
            let _ = self.repo.update_operation_state(
                &op_id,
                OperationState::Recovering,
                Some(err.clone()),
            );
            return Err(err);
        }

        Ok(())
    }

    pub fn launch_game(
        &self,
        game_id: &str,
        setup_id: &str,
        setup_mods_path: &Path,
    ) -> Result<LaunchSession, String> {
        let _guard = self.lock.acquire_guard()?;
        self.ensure_no_pending_operations()?;

        if self.launcher.is_game_running(None) {
            return Err("Stardew Valley is already running".to_string());
        }

        let game = self
            .repo
            .get_game(game_id)?
            .ok_or_else(|| format!("Game '{}' not found", game_id))?;

        let mods = self.repo.list_installed_mods(setup_id)?;
        let expected_mod_ids: Vec<String> = mods.iter().map(|m| m.unique_id.clone()).collect();

        // Baseline log capture
        let mut baseline = self.log_reader.capture_baseline().ok();
        if let Some(ref mut b) = baseline {
            b.expected_mods_path = Some(setup_mods_path.to_path_buf());
        }
        let baseline_time = baseline.as_ref().map(|b| b.launch_time);

        // SMAPI executable and launch args
        let executable = game
            .canonical_root
            .join(crate::smapi::SMAPI_EXECUTABLE_NAME);
        let launch_spec = LaunchSpec {
            executable: executable.clone(),
            args: vec![
                "--mods-path".to_string(),
                setup_mods_path.to_string_lossy().to_string(),
            ],
            working_dir: game.canonical_root.clone(),
            env: Vec::new(),
        };

        let pid = self.launcher.launch_game(&launch_spec)?;

        let session = LaunchSession {
            id: format!("session-{}", uuid_v4()),
            game_id: game_id.to_string(),
            setup_id: setup_id.to_string(),
            launched_at: Utc::now(),
            pid: Some(pid),
            state: SessionState::RunningUnverified,
            expected_mod_ids,
            log_baseline_time: baseline_time,
            log_baseline: baseline,
            verification_result: None,
        };

        self.repo.save_launch_session(&session)?;

        Ok(session)
    }

    pub fn poll_session(&self, session_id: &str) -> Result<Option<LaunchSession>, String> {
        let mut session = match self.repo.get_launch_session(session_id)? {
            Some(s) => s,
            None => return Ok(None),
        };

        // 1. Check if process is still running
        let is_alive = self.launcher.is_game_running(session.pid);

        if !is_alive {
            // Process has exited
            if session.state != SessionState::Exited && session.state != SessionState::Failed {
                if session.state == SessionState::RunningUnverified {
                    // Died prematurely before SMAPI or mod loading could be confirmed
                    session.state = SessionState::Failed;
                    session.verification_result = Some(VerificationResult {
                        confirmed_mods: Vec::new(),
                        details: "Game process terminated prematurely during startup before mod loading could be confirmed.".to_string(),
                        timestamp: Utc::now(),
                    });
                } else {
                    session.state = SessionState::Exited;
                }
                self.repo.update_launch_session(&session)?;
            }
            return Ok(Some(session));
        }

        // 2. Process is still running!
        // If running unverified, attempt log verification
        if session.state == SessionState::RunningUnverified {
            let installed_mods = self.repo.list_installed_mods(&session.setup_id)?;
            let Some(baseline) = session.log_baseline.clone() else {
                session.state = SessionState::VerificationUnavailable;
                self.repo.update_launch_session(&session)?;
                return Ok(Some(session));
            };

            match self.log_reader.verify_session(
                &baseline,
                &session.expected_mod_ids,
                &installed_mods,
            ) {
                Ok(res) if res.all_mods_confirmed => {
                    let confirmed_ids: Vec<String> = res
                        .verified_mods
                        .into_iter()
                        .filter(|m| m.found_in_log)
                        .map(|m| m.unique_id)
                        .collect();
                    session.state = SessionState::ModLoadConfirmed;
                    session.verification_result = Some(VerificationResult {
                        confirmed_mods: confirmed_ids,
                        details: "All expected mods confirmed loaded by SMAPI".to_string(),
                        timestamp: Utc::now(),
                    });
                    self.repo.update_launch_session(&session)?;
                }
                _ => {
                    // Check if elapsed time has exceeded 60s
                    let elapsed = Utc::now().signed_duration_since(session.launched_at);
                    if elapsed.num_seconds() > 60 {
                        session.state = SessionState::VerificationUnavailable;
                        self.repo.update_launch_session(&session)?;
                    }
                }
            }
        }

        Ok(Some(session))
    }

    pub fn terminate_game(&self, session_id: Option<&str>) -> Result<(), String> {
        let pid = if let Some(id) = session_id {
            if let Ok(Some(session)) = self.repo.get_launch_session(id) {
                session.pid
            } else {
                None
            }
        } else {
            None
        };

        self.launcher.terminate_game(pid)?;

        if let Some(id) = session_id {
            if let Ok(Some(mut session)) = self.repo.get_launch_session(id) {
                session.state = SessionState::Exited;
                let _ = self.repo.update_launch_session(&session);
            }
        } else if let Ok(Some(mut session)) = self.repo.get_latest_launch_session(None) {
            if session.state != SessionState::Exited && session.state != SessionState::Failed {
                session.state = SessionState::Exited;
                let _ = self.repo.update_launch_session(&session);
            }
        }

        Ok(())
    }

    pub fn get_smapi_log(&self) -> Result<String, String> {
        self.log_reader.read_log_content()
    }

    pub fn get_smapi_log_path(&self) -> PathBuf {
        self.log_reader.log_file_path()
    }

    pub fn recover_operations(
        &self,
        mods_dir: &Path,
        staging_dir: &Path,
        recovery_dir: &Path,
    ) -> Result<usize, String> {
        self.recover_operations_with_resolver(|_| {
            (
                mods_dir.to_path_buf(),
                staging_dir.to_path_buf(),
                recovery_dir.to_path_buf(),
            )
        })
    }

    pub fn recover_operations_with_resolver<F>(&self, path_resolver: F) -> Result<usize, String>
    where
        F: Fn(&str) -> (PathBuf, PathBuf, PathBuf),
    {
        let _guard = self.lock.acquire_guard()?;
        if self.launcher.is_game_running(None) {
            return Err("Game is running; recovery deferred until it exits".into());
        }
        let mut count = 0;
        for op in self.repo.list_unresolved_operations()? {
            match op.kind {
                OperationKind::ModInstall => {
                    let plan: InstallPlan = serde_json::from_str(&op.plan_json)
                        .map_err(|e| format!("Invalid recovery journal: {e}"))?;
                    crate::install::validate_relative_path(&plan.setup_id)?;
                    crate::install::validate_relative_path(&plan.plan_id)?;
                    crate::install::validate_relative_path(&plan.mod_folder_name)?;
                    let (mods, staging, _) = path_resolver(&plan.setup_id);
                    let dest = mods.join(&plan.mod_folder_name);
                    let staged = staging.join(&plan.plan_id);
                    if dest.exists() {
                        plan.verify_staged_content(&dest)?;
                        let components = if plan.component_manifests.is_empty() {
                            vec![crate::install::ComponentManifest {
                                manifest: plan.manifest.clone(),
                                raw_manifest: plan.raw_manifest.clone(),
                                relative_subfolder: String::new(),
                            }]
                        } else {
                            plan.component_manifests.clone()
                        };
                        let mut items = Vec::new();
                        for component in components {
                            if !component.relative_subfolder.is_empty() {
                                crate::install::validate_relative_path(
                                    &component.relative_subfolder,
                                )?;
                            }
                            let m = component.manifest;
                            items.push(InstalledMod {
                                id: format!("mod-{}", uuid_v4()),
                                setup_id: plan.setup_id.clone(),
                                package_id: plan.package_hash.clone(),
                                unique_id: m.unique_id.to_string(),
                                name: m.name,
                                author: m.author,
                                version: m.version,
                                description: m.description,
                                raw_manifest: component.raw_manifest,
                                relative_target_path: if component.relative_subfolder.is_empty() {
                                    plan.mod_folder_name.clone()
                                } else {
                                    format!(
                                        "{}/{}",
                                        plan.mod_folder_name, component.relative_subfolder
                                    )
                                },
                                file_inventory: plan.file_inventory.clone(),
                                installed_at: Utc::now(),
                            });
                        }
                        self.repo
                            .commit_bundle_install_transaction(&op.id, &items)?;
                    } else {
                        if staged.exists() {
                            std::fs::remove_dir_all(&staged).map_err(|e| e.to_string())?;
                        }
                        self.repo.update_operation_state(
                            &op.id,
                            OperationState::Failed,
                            Some("Installation interrupted before publication".into()),
                        )?;
                    }
                }
                OperationKind::ModRemove => {
                    let plan: RemovalPlan = serde_json::from_str(&op.plan_json)
                        .map_err(|e| format!("Invalid removal journal: {e}"))?;
                    crate::install::validate_relative_path(&plan.setup_id)?;
                    crate::install::validate_relative_path(&plan.relative_folder_path)?;
                    crate::install::validate_relative_path(&op.id)?;
                    let (mods, _, recovery) = path_resolver(&plan.setup_id);
                    let source = mods.join(&plan.relative_folder_path);
                    let quarantine = recovery.join(&op.id).join(&plan.relative_folder_path);
                    // Legacy callers can supply the recorded quarantine root; never trust an arbitrary absolute journal path.
                    if quarantine != plan.recovery_folder_path {
                        return Err("Removal recovery path does not match journal".into());
                    }
                    if source.exists() && quarantine.exists() {
                        return Err(
                            "Conflicting removal directories; recovery requires inspection".into(),
                        );
                    }
                    if quarantine.exists() {
                        let ids = if plan.bundle_mod_ids.is_empty() {
                            vec![plan.installed_mod_id]
                        } else {
                            plan.bundle_mod_ids
                        };
                        self.repo.commit_bundle_remove_transaction(&op.id, &ids)?;
                    } else if source.exists() {
                        self.repo.update_operation_state(
                            &op.id,
                            OperationState::Failed,
                            Some("Removal interrupted before moving files".into()),
                        )?;
                    } else {
                        return Err("Removal files missing; journal retained for recovery".into());
                    }
                }
                _ => {
                    return Err(format!(
                        "Interrupted {:?} requires installer reconciliation; operation {} retained",
                        op.kind, op.id
                    ));
                }
            }
            count += 1;
        }
        Ok(count)
    }
}

pub fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
