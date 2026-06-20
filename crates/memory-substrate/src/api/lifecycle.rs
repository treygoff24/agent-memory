//! Substrate lifecycle: init, open, adopt-clone, doctor, and the shared
//! `open_with_options` constructor.

use super::*;

impl Substrate {
    /// Initialize a new memory repository and open it.
    ///
    /// Q4: `git::adopt_clone` is the sole authority that mints
    /// `local-device.yaml`; `init` drives that path so a fresh repo's first
    /// open has a valid device identity in place. Tests / daemons that want to
    /// supply their own device id thread it through `InitOptions::device_id`,
    /// which is forwarded to `git::adopt_clone_explicit`.
    pub async fn init(roots: Roots, options: InitOptions) -> Result<Self, OpenError> {
        let merge_driver = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("memory-merge-driver"));
        git::init_git_repo(&roots.repo, &merge_driver).map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        std::fs::create_dir_all(&roots.runtime)?;
        // Mint device identity via git::adopt_clone_explicit (Q4 authority).
        crate::git::adopt_clone_explicit(&roots.repo, &roots.runtime, &merge_driver, options.device_id, false)
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        // Seed a minimal config.yaml so `open_with_options` can load the active
        // embedding triple.  Deferred: `InitOptions` should carry an explicit
        // `active_embedding` field so callers control the triple.
        write_initial_config_if_absent(&roots.repo)?;
        Self::open_with_options(roots, options.force_unsafe_durability).await
    }

    /// Open an existing substrate.
    pub async fn open(roots: Roots) -> Result<Self, OpenError> {
        Self::open_with_options(roots, false).await
    }

    /// Adopt a cloned repo and open it.
    ///
    /// Q4: `git::adopt_clone` mints `local-device.yaml`. When
    /// `force_new_device` is set, the prior identity file is removed first so
    /// `adopt_clone`'s skip-if-exists guard mints a fresh one.
    pub async fn adopt_clone(roots: Roots, options: AdoptOptions) -> Result<Self, OpenError> {
        if options.force_new_device {
            let local_device = roots.runtime.join("local-device.yaml");
            if local_device.exists() {
                std::fs::remove_file(local_device)?;
            }
        }
        let merge_driver = options
            .merge_driver_path
            .ok_or_else(|| OpenError::InvalidRoots("adopt_clone requires explicit merge_driver_path".to_string()))?;
        git::adopt_clone(&roots.repo, &roots.runtime, &merge_driver)
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        Self::open(roots).await
    }

    /// Doctor report.
    pub async fn doctor(&self) -> DoctorReport {
        let validation = validate_tree(&self.roots.repo, TreeValidationMode::PartialSync);
        let mut report =
            DoctorReport { durability_tier: self.durability, warnings: Vec::new(), repairs_required: Vec::new() };
        if let Err(err) = validation {
            report.repairs_required.push(err.to_string());
        }
        report
    }

    async fn open_with_options(roots: Roots, force_unsafe_durability: bool) -> Result<Self, OpenError> {
        if !has_substrate_marker(&roots.repo) {
            return Err(OpenError::NotAMemorumSubstrate { path: roots.repo.clone() });
        }
        std::fs::create_dir_all(&roots.runtime)?;
        let durability = probe_durability(&roots.repo, force_unsafe_durability);
        if matches!(durability, DurabilityTier::Refused) && !force_unsafe_durability {
            return Err(OpenError::DurabilityUnsupported { tier: durability });
        }
        let device_id = load_device_id(&roots.runtime)?;
        let event_log = roots.repo.join("events").join(format!("{device_id}.jsonl"));
        let startup_reconcile_report = reconcile_startup_pre_index_report(&roots.runtime, &event_log, &roots.repo)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        let device = DeviceId::try_new(&device_id)
            .map_err(|err| OpenError::InvalidRoots(format!("invalid device id in local-device.yaml: {err}")))?;
        sync_event_sequence_state(&roots.runtime, &event_log, &device)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        // `load_active_embedding` returns Err when config.yaml is absent or has
        // no `active_embedding` field.  Spec §10.2.2 #5: no silent fallback.
        // Deferred: introduce typed `OpenError::ActiveEmbeddingTripleRequired` variant.
        let active_embedding = crate::config::load_active_embedding(&roots.repo)
            .map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        let connection =
            open_index(&roots.runtime.join("index.sqlite")).map_err(|err| OpenError::InvalidRoots(err.to_string()))?;
        let mut index = Index::with_active_embedding(connection, active_embedding);
        let startup_reconcile_report = replay_pending_repairs_into_report(
            &roots.repo,
            &roots.runtime,
            &event_log,
            &device,
            &mut index,
            startup_reconcile_report,
        )
        .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        // Plaintext freshness is handled by phase-6 stale detection
        // (`reindex_stale_memories`), which already ran inside
        // `replay_pending_repairs_into_report` above and still reads+hashes each
        // plaintext `.md` once. Here we only run the remaining incremental open
        // sweep: orphan-row cleanup, encrypted-tier reindex, and embedding-job
        // reconciliation — avoiding the old duplicate clear+rebuild pass.
        incremental_reindex_at_open(&roots.repo, &mut index)
            .map_err(|err| OpenError::OperatorRepairRequired(err.to_string()))?;
        match read_all_event_logs_from_repo(&roots.repo).and_then(|events| {
            index.rebuild_events_log_mirror(&events).map_err(|err| std::io::Error::other(err.to_string()))
        }) {
            Ok(()) => {}
            Err(err) => tracing::warn!("events_log SQLite mirror rebuild during open failed: {err}"),
        }
        Ok(Self {
            roots,
            device_id,
            durability,
            index: Arc::new(Mutex::new(index)),
            best_effort_event_seq: Arc::new(AtomicU64::new(best_effort_event_seq_start(&event_log, &device))),
            event_log,
            suppression: Arc::new(Mutex::new(SuppressionLedger::default())),
            startup_reconcile_report: Arc::new(startup_reconcile_report),
        })
    }
}
