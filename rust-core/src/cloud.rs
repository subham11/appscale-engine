//! Cloud Build Service — Remote CI/CD, OTA Updates, Build Artifact Caching.
//!
//! Provides the Rust-side types and orchestration for:
//! 1. **Remote CI/CD**: Define build pipelines targeting multiple platforms,
//!    track build jobs, and collect results.
//! 2. **OTA Updates**: Version-aware update manifests, bundle diffing metadata,
//!    and rollback support for hot-updating JS bundles without app store releases.
//! 3. **Build Artifact Caching**: Content-addressed cache for intermediate build
//!    outputs (compiled Rust dylibs, bundled JS, assets) with TTL and invalidation.
//!
//! This module does NOT perform network I/O — it defines the data model and
//! validation logic. Actual HTTP transport is handled by platform adapters.

use crate::platform::PlatformId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Errors
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("Build failed for target {target:?}: {reason}")]
    BuildFailed { target: BuildTarget, reason: String },

    #[error("Invalid build config: {0}")]
    InvalidConfig(String),

    #[error("Cache miss: {0}")]
    CacheMiss(String),

    #[error("OTA update rejected: {0}")]
    OtaRejected(String),

    #[error("Version conflict: current={current}, required={required}")]
    VersionConflict { current: String, required: String },

    #[error("Cloud service error: {0}")]
    ServiceError(String),
}

pub type CloudResult<T> = Result<T, CloudError>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 1. Remote CI/CD — Build Pipeline
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Platforms a build can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildTarget {
    Ios,
    Android,
    Macos,
    Windows,
    Web,
    Linux,
}

impl BuildTarget {
    pub fn to_platform_id(&self) -> Option<PlatformId> {
        match self {
            BuildTarget::Ios => Some(PlatformId::Ios),
            BuildTarget::Android => Some(PlatformId::Android),
            BuildTarget::Macos => Some(PlatformId::Macos),
            BuildTarget::Windows => Some(PlatformId::Windows),
            BuildTarget::Web => Some(PlatformId::Web),
            BuildTarget::Linux => None, // PlatformId doesn't include Linux yet
        }
    }
}

/// Build mode (debug vs release).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildMode {
    Debug,
    Release,
    Profile,
}

/// Status of a build job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildStatus {
    Queued,
    Running,
    Succeeded,
    Failed { reason: String },
    Cancelled,
}

impl BuildStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            BuildStatus::Succeeded | BuildStatus::Failed { .. } | BuildStatus::Cancelled
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, BuildStatus::Succeeded)
    }
}

/// Configuration for a single build job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildJobConfig {
    pub target: BuildTarget,
    pub mode: BuildMode,
    pub env_vars: HashMap<String, String>,
    pub features: Vec<String>,
    pub signing_profile: Option<String>,
}

/// A single build job within a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildJob {
    pub id: String,
    pub config: BuildJobConfig,
    pub status: BuildStatus,
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
    pub artifact_key: Option<String>,
    pub log_url: Option<String>,
}

impl BuildJob {
    pub fn new(id: impl Into<String>, config: BuildJobConfig) -> Self {
        Self {
            id: id.into(),
            config,
            status: BuildStatus::Queued,
            started_at: None,
            finished_at: None,
            artifact_key: None,
            log_url: None,
        }
    }

    pub fn duration_ms(&self) -> Option<f64> {
        match (self.started_at, self.finished_at) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        }
    }
}

/// A build pipeline targeting one or more platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildPipeline {
    pub id: String,
    pub app_name: String,
    pub app_version: String,
    pub commit_sha: Option<String>,
    pub jobs: Vec<BuildJob>,
    pub created_at: f64,
}

impl BuildPipeline {
    pub fn new(
        id: impl Into<String>,
        app_name: impl Into<String>,
        app_version: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            app_name: app_name.into(),
            app_version: app_version.into(),
            commit_sha: None,
            jobs: Vec::new(),
            created_at: 0.0,
        }
    }

    /// Add a build job for a target platform.
    pub fn add_job(&mut self, config: BuildJobConfig) -> &BuildJob {
        let job_id = format!("{}-{:?}-{}", self.id, config.target, self.jobs.len());
        let job = BuildJob::new(job_id, config);
        self.jobs.push(job);
        self.jobs.last().unwrap()
    }

    /// Check if all jobs have completed (success or failure).
    pub fn is_complete(&self) -> bool {
        !self.jobs.is_empty() && self.jobs.iter().all(|j| j.status.is_terminal())
    }

    /// Check if all jobs succeeded.
    pub fn all_succeeded(&self) -> bool {
        !self.jobs.is_empty() && self.jobs.iter().all(|j| j.status.is_success())
    }

    /// Get jobs by status.
    pub fn jobs_with_status(&self, status_match: &BuildStatus) -> Vec<&BuildJob> {
        self.jobs
            .iter()
            .filter(|j| std::mem::discriminant(&j.status) == std::mem::discriminant(status_match))
            .collect()
    }

    /// Get the set of targets in this pipeline.
    pub fn targets(&self) -> Vec<BuildTarget> {
        self.jobs.iter().map(|j| j.config.target).collect()
    }
}

/// Validates a build pipeline configuration before submission.
pub fn validate_pipeline(pipeline: &BuildPipeline) -> CloudResult<()> {
    if pipeline.app_name.is_empty() {
        return Err(CloudError::InvalidConfig("app_name is required".into()));
    }
    if pipeline.app_version.is_empty() {
        return Err(CloudError::InvalidConfig("app_version is required".into()));
    }
    if pipeline.jobs.is_empty() {
        return Err(CloudError::InvalidConfig(
            "pipeline must have at least one job".into(),
        ));
    }

    // Check for duplicate targets with same mode
    let mut seen = std::collections::HashSet::new();
    for job in &pipeline.jobs {
        let key = (job.config.target, job.config.mode);
        if !seen.insert(key) {
            return Err(CloudError::InvalidConfig(format!(
                "Duplicate target {:?} with mode {:?}",
                key.0, key.1
            )));
        }
    }

    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 2. OTA Updates
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Semantic version for bundles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: Option<u32>,
}

impl BundleVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            build: None,
        }
    }

    pub fn with_build(mut self, build: u32) -> Self {
        self.build = Some(build);
        self
    }

    /// Check if this version is compatible with a minimum required version.
    /// Compatible if major matches and self >= min.
    pub fn is_compatible_with(&self, min: &BundleVersion) -> bool {
        if self.major != min.major {
            return false;
        }
        if self.minor > min.minor {
            return true;
        }
        if self.minor == min.minor {
            return self.patch >= min.patch;
        }
        false
    }
}

impl std::fmt::Display for BundleVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(build) = self.build {
            write!(f, "+{}", build)?;
        }
        Ok(())
    }
}

impl Ord for BundleVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then(self.build.unwrap_or(0).cmp(&other.build.unwrap_or(0)))
    }
}

impl PartialOrd for BundleVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A single OTA update entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaUpdate {
    pub version: BundleVersion,
    pub min_native_version: BundleVersion,
    pub bundle_url: String,
    pub bundle_hash: String,
    pub bundle_size_bytes: u64,
    pub release_notes: String,
    pub is_mandatory: bool,
    pub rollout_percentage: u8,
    pub created_at: f64,
    pub target_platforms: Vec<BuildTarget>,
}

/// Manifest listing available OTA updates for an app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaManifest {
    pub app_id: String,
    pub updates: Vec<OtaUpdate>,
}

impl OtaManifest {
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            updates: Vec::new(),
        }
    }

    /// Find the latest compatible update for the given native version and platform.
    pub fn latest_update(
        &self,
        current_native: &BundleVersion,
        platform: BuildTarget,
    ) -> Option<&OtaUpdate> {
        self.updates
            .iter()
            .filter(|u| current_native.is_compatible_with(&u.min_native_version))
            .filter(|u| u.target_platforms.contains(&platform))
            .max_by(|a, b| a.version.cmp(&b.version))
    }

    /// Check if an update is available for the given version.
    pub fn has_update_for(
        &self,
        current_bundle: &BundleVersion,
        current_native: &BundleVersion,
        platform: BuildTarget,
    ) -> bool {
        self.latest_update(current_native, platform)
            .map(|u| u.version > *current_bundle)
            .unwrap_or(false)
    }
}

/// Decision about whether to apply an OTA update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtaDecision {
    /// No update available.
    NoUpdate,
    /// Update available but not mandatory — user choice.
    Optional { version: BundleVersion },
    /// Mandatory update — must apply before continuing.
    Mandatory { version: BundleVersion },
    /// Update available but native version is too old — needs app store update.
    NativeUpdateRequired { min_native: BundleVersion },
}

/// Evaluate whether an OTA update should be applied.
pub fn evaluate_ota(
    manifest: &OtaManifest,
    current_bundle: &BundleVersion,
    current_native: &BundleVersion,
    platform: BuildTarget,
) -> OtaDecision {
    // Check if any update requires a newer native version
    let newest = manifest
        .updates
        .iter()
        .filter(|u| u.target_platforms.contains(&platform))
        .max_by(|a, b| a.version.cmp(&b.version));

    let Some(newest) = newest else {
        return OtaDecision::NoUpdate;
    };

    // If already on latest or newer, no update
    if current_bundle >= &newest.version {
        return OtaDecision::NoUpdate;
    }

    // If native version isn't compatible with the latest
    if !current_native.is_compatible_with(&newest.min_native_version) {
        return OtaDecision::NativeUpdateRequired {
            min_native: newest.min_native_version.clone(),
        };
    }

    // Find the best compatible update
    match manifest.latest_update(current_native, platform) {
        Some(update) if update.version > *current_bundle => {
            if update.is_mandatory {
                OtaDecision::Mandatory {
                    version: update.version.clone(),
                }
            } else {
                OtaDecision::Optional {
                    version: update.version.clone(),
                }
            }
        }
        _ => OtaDecision::NoUpdate,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 3. Build Artifact Caching
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A content-addressed cache key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheKey {
    /// Hash of the inputs (source files, deps, config).
    pub content_hash: String,
    /// Target platform.
    pub target: BuildTarget,
    /// Build mode.
    pub mode: BuildMode,
}

impl CacheKey {
    pub fn new(content_hash: impl Into<String>, target: BuildTarget, mode: BuildMode) -> Self {
        Self {
            content_hash: content_hash.into(),
            target,
            mode,
        }
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}-{:?}-{}", self.target, self.mode, self.content_hash)
    }
}

/// A cached build artifact entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub key: CacheKey,
    pub artifact_path: String,
    pub size_bytes: u64,
    pub created_at: f64,
    pub last_accessed: f64,
    pub ttl_hours: u32,
    pub metadata: HashMap<String, String>,
}

impl CacheEntry {
    /// Check if this entry has expired (based on creation time and TTL).
    pub fn is_expired(&self, now: f64) -> bool {
        let ttl_ms = self.ttl_hours as f64 * 3600.0 * 1000.0;
        now - self.created_at > ttl_ms
    }
}

/// In-memory artifact cache with LRU eviction.
pub struct ArtifactCache {
    entries: HashMap<CacheKey, CacheEntry>,
    max_size_bytes: u64,
    current_size_bytes: u64,
}

impl ArtifactCache {
    pub fn new(max_size_bytes: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_size_bytes,
            current_size_bytes: 0,
        }
    }

    /// Look up an artifact by cache key.
    pub fn get(&mut self, key: &CacheKey, now: f64) -> Option<&CacheEntry> {
        // Remove if expired
        if let Some(entry) = self.entries.get(key) {
            if entry.is_expired(now) {
                let size = entry.size_bytes;
                self.entries.remove(key);
                self.current_size_bytes = self.current_size_bytes.saturating_sub(size);
                return None;
            }
        }
        // Update last_accessed
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_accessed = now;
        }
        self.entries.get(key)
    }

    /// Insert an artifact into the cache. Evicts oldest entries if needed.
    pub fn insert(&mut self, entry: CacheEntry) {
        // Evict if necessary
        while self.current_size_bytes + entry.size_bytes > self.max_size_bytes
            && !self.entries.is_empty()
        {
            self.evict_oldest();
        }

        // Don't cache if single entry exceeds max
        if entry.size_bytes > self.max_size_bytes {
            return;
        }

        self.current_size_bytes += entry.size_bytes;
        self.entries.insert(entry.key.clone(), entry);
    }

    /// Remove an entry by key.
    pub fn remove(&mut self, key: &CacheKey) -> Option<CacheEntry> {
        let entry = self.entries.remove(key)?;
        self.current_size_bytes = self.current_size_bytes.saturating_sub(entry.size_bytes);
        Some(entry)
    }

    /// Remove all expired entries.
    pub fn purge_expired(&mut self, now: f64) -> usize {
        let expired_keys: Vec<CacheKey> = self
            .entries
            .iter()
            .filter(|(_, e)| e.is_expired(now))
            .map(|(k, _)| k.clone())
            .collect();
        let count = expired_keys.len();
        for key in expired_keys {
            self.remove(&key);
        }
        count
    }

    /// Evict the least-recently-accessed entry.
    fn evict_oldest(&mut self) {
        let oldest_key = self
            .entries
            .iter()
            .min_by(|a, b| {
                a.1.last_accessed
                    .partial_cmp(&b.1.last_accessed)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(k, _)| k.clone());
        if let Some(key) = oldest_key {
            self.remove(&key);
        }
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Current cache size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.current_size_bytes
    }

    /// Cache utilization as a fraction (0.0 – 1.0).
    pub fn utilization(&self) -> f64 {
        if self.max_size_bytes == 0 {
            return 0.0;
        }
        self.current_size_bytes as f64 / self.max_size_bytes as f64
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    // ── Build Pipeline Tests ──

    #[test]
    fn test_pipeline_creation_and_validation() {
        let mut pipeline = BuildPipeline::new("p1", "MyApp", "1.0.0");
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Ios,
            mode: BuildMode::Release,
            env_vars: HashMap::new(),
            features: vec!["push".into()],
            signing_profile: Some("dist".into()),
        });
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Android,
            mode: BuildMode::Release,
            env_vars: HashMap::new(),
            features: vec![],
            signing_profile: None,
        });

        assert_eq!(pipeline.jobs.len(), 2);
        assert!(!pipeline.is_complete());
        assert!(validate_pipeline(&pipeline).is_ok());
    }

    #[test]
    fn test_pipeline_validation_empty_name() {
        let pipeline = BuildPipeline::new("p1", "", "1.0.0");
        assert!(validate_pipeline(&pipeline).is_err());
    }

    #[test]
    fn test_pipeline_validation_no_jobs() {
        let pipeline = BuildPipeline::new("p1", "App", "1.0.0");
        assert!(validate_pipeline(&pipeline).is_err());
    }

    #[test]
    fn test_pipeline_validation_duplicate_targets() {
        let mut pipeline = BuildPipeline::new("p1", "App", "1.0.0");
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Ios,
            mode: BuildMode::Release,
            env_vars: HashMap::new(),
            features: vec![],
            signing_profile: None,
        });
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Ios,
            mode: BuildMode::Release,
            env_vars: HashMap::new(),
            features: vec![],
            signing_profile: None,
        });
        assert!(validate_pipeline(&pipeline).is_err());
    }

    #[test]
    fn test_build_status_terminal() {
        assert!(!BuildStatus::Queued.is_terminal());
        assert!(!BuildStatus::Running.is_terminal());
        assert!(BuildStatus::Succeeded.is_terminal());
        assert!(BuildStatus::Failed { reason: "e".into() }.is_terminal());
        assert!(BuildStatus::Cancelled.is_terminal());
    }

    #[test]
    fn test_pipeline_completion_tracking() {
        let mut pipeline = BuildPipeline::new("p1", "App", "1.0.0");
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Web,
            mode: BuildMode::Debug,
            env_vars: HashMap::new(),
            features: vec![],
            signing_profile: None,
        });
        assert!(!pipeline.is_complete());

        pipeline.jobs[0].status = BuildStatus::Succeeded;
        assert!(pipeline.is_complete());
        assert!(pipeline.all_succeeded());

        pipeline.jobs[0].status = BuildStatus::Failed {
            reason: "oom".into(),
        };
        assert!(pipeline.is_complete());
        assert!(!pipeline.all_succeeded());
    }

    // ── OTA Update Tests ──

    #[test]
    fn test_version_ordering() {
        let v1 = BundleVersion::new(1, 0, 0);
        let v1_1 = BundleVersion::new(1, 1, 0);
        let v2 = BundleVersion::new(2, 0, 0);

        assert!(v1 < v1_1);
        assert!(v1_1 < v2);
        assert!(v1 < v2);
    }

    #[test]
    fn test_version_compatibility() {
        let v1_2_0 = BundleVersion::new(1, 2, 0);
        let v1_1_0 = BundleVersion::new(1, 1, 0);
        let v1_3_0 = BundleVersion::new(1, 3, 0);
        let v2_0_0 = BundleVersion::new(2, 0, 0);

        // Same major, self >= min → compatible
        assert!(v1_2_0.is_compatible_with(&v1_1_0));
        assert!(v1_2_0.is_compatible_with(&v1_2_0));
        // Self < min → incompatible
        assert!(!v1_1_0.is_compatible_with(&v1_2_0));
        // Different major → incompatible
        assert!(!v2_0_0.is_compatible_with(&v1_3_0));
        assert!(!v1_3_0.is_compatible_with(&v2_0_0));
    }

    #[test]
    fn test_version_display() {
        assert_eq!(BundleVersion::new(1, 2, 3).to_string(), "1.2.3");
        assert_eq!(
            BundleVersion::new(1, 0, 0).with_build(42).to_string(),
            "1.0.0+42"
        );
    }

    #[test]
    fn test_ota_no_updates() {
        let manifest = OtaManifest::new("app1");
        let current = BundleVersion::new(1, 0, 0);
        let native = BundleVersion::new(1, 0, 0);
        assert_eq!(
            evaluate_ota(&manifest, &current, &native, BuildTarget::Ios),
            OtaDecision::NoUpdate
        );
    }

    #[test]
    fn test_ota_optional_update() {
        let mut manifest = OtaManifest::new("app1");
        manifest.updates.push(OtaUpdate {
            version: BundleVersion::new(1, 1, 0),
            min_native_version: BundleVersion::new(1, 0, 0),
            bundle_url: "https://cdn.example.com/bundle-1.1.0.js".into(),
            bundle_hash: "abc123".into(),
            bundle_size_bytes: 1024,
            release_notes: "Bug fixes".into(),
            is_mandatory: false,
            rollout_percentage: 100,
            created_at: 1000.0,
            target_platforms: vec![BuildTarget::Ios, BuildTarget::Android],
        });

        let current = BundleVersion::new(1, 0, 0);
        let native = BundleVersion::new(1, 0, 0);
        assert_eq!(
            evaluate_ota(&manifest, &current, &native, BuildTarget::Ios),
            OtaDecision::Optional {
                version: BundleVersion::new(1, 1, 0)
            }
        );
    }

    #[test]
    fn test_ota_mandatory_update() {
        let mut manifest = OtaManifest::new("app1");
        manifest.updates.push(OtaUpdate {
            version: BundleVersion::new(1, 2, 0),
            min_native_version: BundleVersion::new(1, 0, 0),
            bundle_url: "https://cdn.example.com/b.js".into(),
            bundle_hash: "def456".into(),
            bundle_size_bytes: 2048,
            release_notes: "Critical fix".into(),
            is_mandatory: true,
            rollout_percentage: 100,
            created_at: 2000.0,
            target_platforms: vec![BuildTarget::Ios],
        });

        let current = BundleVersion::new(1, 0, 0);
        let native = BundleVersion::new(1, 0, 0);
        assert_eq!(
            evaluate_ota(&manifest, &current, &native, BuildTarget::Ios),
            OtaDecision::Mandatory {
                version: BundleVersion::new(1, 2, 0)
            }
        );
    }

    #[test]
    fn test_ota_native_update_required() {
        let mut manifest = OtaManifest::new("app1");
        manifest.updates.push(OtaUpdate {
            version: BundleVersion::new(2, 0, 0),
            min_native_version: BundleVersion::new(2, 0, 0),
            bundle_url: "https://cdn.example.com/b.js".into(),
            bundle_hash: "ghi789".into(),
            bundle_size_bytes: 4096,
            release_notes: "Major update".into(),
            is_mandatory: true,
            rollout_percentage: 100,
            created_at: 3000.0,
            target_platforms: vec![BuildTarget::Ios],
        });

        let current = BundleVersion::new(1, 0, 0);
        let native = BundleVersion::new(1, 0, 0);
        assert_eq!(
            evaluate_ota(&manifest, &current, &native, BuildTarget::Ios),
            OtaDecision::NativeUpdateRequired {
                min_native: BundleVersion::new(2, 0, 0)
            }
        );
    }

    #[test]
    fn test_ota_already_latest() {
        let mut manifest = OtaManifest::new("app1");
        manifest.updates.push(OtaUpdate {
            version: BundleVersion::new(1, 0, 0),
            min_native_version: BundleVersion::new(1, 0, 0),
            bundle_url: "https://cdn.example.com/b.js".into(),
            bundle_hash: "x".into(),
            bundle_size_bytes: 100,
            release_notes: "".into(),
            is_mandatory: false,
            rollout_percentage: 100,
            created_at: 0.0,
            target_platforms: vec![BuildTarget::Ios],
        });

        let current = BundleVersion::new(1, 0, 0);
        let native = BundleVersion::new(1, 0, 0);
        assert_eq!(
            evaluate_ota(&manifest, &current, &native, BuildTarget::Ios),
            OtaDecision::NoUpdate
        );
    }

    // ── Artifact Cache Tests ──

    #[test]
    fn test_cache_insert_and_get() {
        let mut cache = ArtifactCache::new(1_000_000);
        let key = CacheKey::new("abc123", BuildTarget::Ios, BuildMode::Release);
        let entry = CacheEntry {
            key: key.clone(),
            artifact_path: "/tmp/build.ipa".into(),
            size_bytes: 5000,
            created_at: 1000.0,
            last_accessed: 1000.0,
            ttl_hours: 24,
            metadata: HashMap::new(),
        };
        cache.insert(entry);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.size_bytes(), 5000);
        let hit = cache.get(&key, 2000.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().artifact_path, "/tmp/build.ipa");
    }

    #[test]
    fn test_cache_expiration() {
        let mut cache = ArtifactCache::new(1_000_000);
        let key = CacheKey::new("abc", BuildTarget::Web, BuildMode::Debug);
        let entry = CacheEntry {
            key: key.clone(),
            artifact_path: "/tmp/out".into(),
            size_bytes: 100,
            created_at: 0.0,
            last_accessed: 0.0,
            ttl_hours: 1,
            metadata: HashMap::new(),
        };
        cache.insert(entry);

        // Within TTL
        assert!(cache.get(&key, 3_000_000.0).is_some());
        // After TTL (1 hour = 3_600_000 ms)
        assert!(cache.get(&key, 3_700_000.0).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = ArtifactCache::new(100);
        for i in 0..5 {
            let key = CacheKey::new(format!("hash{i}"), BuildTarget::Android, BuildMode::Release);
            let entry = CacheEntry {
                key,
                artifact_path: format!("/tmp/out{i}"),
                size_bytes: 30,
                created_at: i as f64 * 1000.0,
                last_accessed: i as f64 * 1000.0,
                ttl_hours: 24,
                metadata: HashMap::new(),
            };
            cache.insert(entry);
        }

        // Max 100 bytes, each entry 30 bytes → at most 3 entries
        assert!(cache.len() <= 3);
        assert!(cache.size_bytes() <= 100);
    }

    #[test]
    fn test_cache_purge_expired() {
        let mut cache = ArtifactCache::new(1_000_000);
        for i in 0..3 {
            let key = CacheKey::new(format!("h{i}"), BuildTarget::Macos, BuildMode::Debug);
            let entry = CacheEntry {
                key,
                artifact_path: format!("/tmp/{i}"),
                size_bytes: 10,
                created_at: 0.0,
                last_accessed: 0.0,
                ttl_hours: 1,
                metadata: HashMap::new(),
            };
            cache.insert(entry);
        }

        assert_eq!(cache.len(), 3);
        let purged = cache.purge_expired(4_000_000.0);
        assert_eq!(purged, 3);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_cache_oversize_entry_rejected() {
        let mut cache = ArtifactCache::new(100);
        let key = CacheKey::new("big", BuildTarget::Linux, BuildMode::Release);
        let entry = CacheEntry {
            key: key.clone(),
            artifact_path: "/tmp/huge".into(),
            size_bytes: 200,
            created_at: 0.0,
            last_accessed: 0.0,
            ttl_hours: 24,
            metadata: HashMap::new(),
        };
        cache.insert(entry);
        assert_eq!(cache.len(), 0); // rejected — too big
    }

    #[test]
    fn test_cache_utilization() {
        let mut cache = ArtifactCache::new(1000);
        assert_eq!(cache.utilization(), 0.0);

        let key = CacheKey::new("x", BuildTarget::Web, BuildMode::Release);
        cache.insert(CacheEntry {
            key,
            artifact_path: "/tmp/x".into(),
            size_bytes: 500,
            created_at: 0.0,
            last_accessed: 0.0,
            ttl_hours: 24,
            metadata: HashMap::new(),
        });
        assert!((cache.utilization() - 0.5).abs() < 0.001);
    }

    // ── Serialization Tests ──

    #[test]
    fn test_build_pipeline_serialization() {
        let mut pipeline = BuildPipeline::new("p1", "TestApp", "2.0.0");
        pipeline.add_job(BuildJobConfig {
            target: BuildTarget::Ios,
            mode: BuildMode::Release,
            env_vars: HashMap::new(),
            features: vec!["analytics".into()],
            signing_profile: Some("dist".into()),
        });

        let json = serde_json::to_string(&pipeline).unwrap();
        let restored: BuildPipeline = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.app_name, "TestApp");
        assert_eq!(restored.jobs.len(), 1);
        assert_eq!(restored.jobs[0].config.target, BuildTarget::Ios);
    }

    #[test]
    fn test_ota_manifest_serialization() {
        let mut manifest = OtaManifest::new("com.example.app");
        manifest.updates.push(OtaUpdate {
            version: BundleVersion::new(1, 0, 1),
            min_native_version: BundleVersion::new(1, 0, 0),
            bundle_url: "https://cdn.example.com/b.js".into(),
            bundle_hash: "sha256-abc".into(),
            bundle_size_bytes: 512,
            release_notes: "Patch".into(),
            is_mandatory: false,
            rollout_percentage: 50,
            created_at: 100.0,
            target_platforms: vec![BuildTarget::Ios, BuildTarget::Android],
        });

        let json = serde_json::to_string(&manifest).unwrap();
        let restored: OtaManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.app_id, "com.example.app");
        assert_eq!(restored.updates.len(), 1);
        assert_eq!(restored.updates[0].rollout_percentage, 50);
    }

    #[test]
    fn test_build_target_platform_id_mapping() {
        assert_eq!(BuildTarget::Ios.to_platform_id(), Some(PlatformId::Ios));
        assert_eq!(
            BuildTarget::Android.to_platform_id(),
            Some(PlatformId::Android)
        );
        assert_eq!(BuildTarget::Web.to_platform_id(), Some(PlatformId::Web));
        assert_eq!(BuildTarget::Linux.to_platform_id(), None);
    }
}
