//! Plugin Marketplace — Plugin specification, registry, and discovery.
//!
//! Extends the `NativeModule` system with:
//! 1. **Plugin Spec**: Metadata describing a plugin (name, version, platforms,
//!    capabilities, dependencies).
//! 2. **Plugin Registry**: Manages installed plugins, resolves dependencies,
//!    and tracks lifecycle.
//! 3. **Discovery & Compatibility**: Version compatibility checking, platform
//!    support matrix, and search/filtering for a future marketplace UI.
//!
//! Plugins wrap `NativeModule`s with package-level metadata so the framework
//! can manage them as first-class ecosystem citizens.

use crate::modules::{NativeModule, ModuleRegistry, ModuleError};
use crate::cloud::BuildTarget;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Errors
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Plugin already installed: {0} v{1}")]
    AlreadyInstalled(String, String),

    #[error("Incompatible plugin: {plugin} requires engine >={required}, have {current}")]
    IncompatibleEngine { plugin: String, required: String, current: String },

    #[error("Platform not supported: {plugin} does not support {platform:?}")]
    PlatformNotSupported { plugin: String, platform: BuildTarget },

    #[error("Missing dependency: {plugin} requires {dependency}")]
    MissingDependency { plugin: String, dependency: String },

    #[error("Module registration error: {0}")]
    ModuleError(#[from] ModuleError),

    #[error("Plugin error: {0}")]
    Internal(String),
}

pub type PluginResult<T> = Result<T, PluginError>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 1. Plugin Spec
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Semantic version for plugins.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PluginVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl PluginVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    /// Check if this version satisfies a minimum version requirement.
    pub fn satisfies_min(&self, min: &PluginVersion) -> bool {
        self >= min
    }
}

impl std::fmt::Display for PluginVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What a plugin provides to the framework.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginCapability {
    /// Provides a native module callable from JS.
    NativeModule,
    /// Provides UI components.
    Components,
    /// Provides a storage backend.
    Storage,
    /// Provides platform-specific functionality (camera, haptics, etc.).
    PlatformFeature(String),
    /// Provides theme/styling resources.
    Theme,
    /// Provides navigation integration.
    Navigation,
}

/// A dependency on another plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    pub name: String,
    pub min_version: PluginVersion,
}

/// Category for marketplace browsing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginCategory {
    UI,
    Data,
    Media,
    Platform,
    Analytics,
    Auth,
    Networking,
    DevTools,
    Other,
}

/// Full descriptor for a plugin package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: PluginVersion,
    pub description: String,
    pub author: String,
    pub license: String,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub category: PluginCategory,
    pub capabilities: Vec<PluginCapability>,
    pub supported_platforms: Vec<BuildTarget>,
    pub min_engine_version: PluginVersion,
    pub dependencies: Vec<PluginDependency>,
    pub js_package: Option<String>,
    pub keywords: Vec<String>,
}

impl PluginDescriptor {
    /// Check if this plugin supports the given platform.
    pub fn supports_platform(&self, target: &BuildTarget) -> bool {
        self.supported_platforms.contains(target)
    }

    /// Check if this plugin is compatible with the given engine version.
    pub fn is_compatible_with_engine(&self, engine_version: &PluginVersion) -> bool {
        engine_version.satisfies_min(&self.min_engine_version)
    }

    /// Check if all dependencies are satisfied by the given installed set.
    pub fn check_dependencies(&self, installed: &HashMap<String, PluginVersion>) -> Vec<String> {
        let mut missing = Vec::new();
        for dep in &self.dependencies {
            match installed.get(&dep.name) {
                Some(v) if v.satisfies_min(&dep.min_version) => {}
                _ => missing.push(dep.name.clone()),
            }
        }
        missing
    }
}

/// Validates a plugin descriptor for completeness.
pub fn validate_descriptor(desc: &PluginDescriptor) -> PluginResult<()> {
    if desc.name.is_empty() {
        return Err(PluginError::Internal("Plugin name is required".into()));
    }
    if desc.description.is_empty() {
        return Err(PluginError::Internal("Plugin description is required".into()));
    }
    if desc.supported_platforms.is_empty() {
        return Err(PluginError::Internal("At least one supported platform is required".into()));
    }
    if desc.capabilities.is_empty() {
        return Err(PluginError::Internal("At least one capability is required".into()));
    }
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 2. Plugin Registry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// An installed plugin with its descriptor and optional native module.
struct InstalledPlugin {
    descriptor: PluginDescriptor,
    module: Option<Arc<dyn NativeModule>>,
    enabled: bool,
}

/// Registry managing all installed plugins.
pub struct PluginRegistry {
    plugins: HashMap<String, InstalledPlugin>,
    module_registry: ModuleRegistry,
    engine_version: PluginVersion,
}

impl PluginRegistry {
    pub fn new(engine_version: PluginVersion) -> Self {
        Self {
            plugins: HashMap::new(),
            module_registry: ModuleRegistry::new(),
            engine_version,
        }
    }

    /// Install a plugin. Validates compatibility and registers its native module.
    pub fn install(
        &mut self,
        descriptor: PluginDescriptor,
        module: Option<Arc<dyn NativeModule>>,
    ) -> PluginResult<()> {
        validate_descriptor(&descriptor)?;

        // Check engine compatibility
        if !descriptor.is_compatible_with_engine(&self.engine_version) {
            return Err(PluginError::IncompatibleEngine {
                plugin: descriptor.name.clone(),
                required: descriptor.min_engine_version.to_string(),
                current: self.engine_version.to_string(),
            });
        }

        // Check not already installed
        if self.plugins.contains_key(&descriptor.name) {
            return Err(PluginError::AlreadyInstalled(
                descriptor.name.clone(),
                descriptor.version.to_string(),
            ));
        }

        // Check dependencies
        let installed_versions: HashMap<String, PluginVersion> = self.plugins.iter()
            .map(|(k, v)| (k.clone(), v.descriptor.version.clone()))
            .collect();
        let missing = descriptor.check_dependencies(&installed_versions);
        if !missing.is_empty() {
            return Err(PluginError::MissingDependency {
                plugin: descriptor.name.clone(),
                dependency: missing.join(", "),
            });
        }

        // Register native module if provided
        if let Some(ref m) = module {
            self.module_registry.register(m.clone())?;
        }

        let name = descriptor.name.clone();
        self.plugins.insert(name, InstalledPlugin {
            descriptor,
            module,
            enabled: true,
        });

        Ok(())
    }

    /// Uninstall a plugin by name.
    pub fn uninstall(&mut self, name: &str) -> PluginResult<()> {
        // Check no other plugin depends on this one before removing
        let dependent = self.plugins.iter()
            .find(|(_, other)| other.descriptor.dependencies.iter().any(|d| d.name == name))
            .map(|(n, _)| n.clone());
        if let Some(dep_name) = dependent {
            return Err(PluginError::Internal(
                format!("Cannot uninstall '{}': required by '{}'", name, dep_name),
            ));
        }

        let plugin = self.plugins.remove(name)
            .ok_or_else(|| PluginError::NotFound(name.into()))?;

        // Unregister native module
        if plugin.module.is_some() {
            self.module_registry.unregister(name);
        }

        Ok(())
    }

    /// Enable or disable a plugin.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> PluginResult<()> {
        let plugin = self.plugins.get_mut(name)
            .ok_or_else(|| PluginError::NotFound(name.into()))?;
        plugin.enabled = enabled;
        Ok(())
    }

    /// Get a plugin descriptor by name.
    pub fn get_descriptor(&self, name: &str) -> Option<&PluginDescriptor> {
        self.plugins.get(name).map(|p| &p.descriptor)
    }

    /// Check if a plugin is installed.
    pub fn is_installed(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// List all installed plugin names.
    pub fn installed_names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// List enabled plugins.
    pub fn enabled_plugins(&self) -> Vec<&PluginDescriptor> {
        self.plugins.values()
            .filter(|p| p.enabled)
            .map(|p| &p.descriptor)
            .collect()
    }

    /// Number of installed plugins.
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Get the underlying module registry (for bridge calls).
    pub fn module_registry(&self) -> &ModuleRegistry {
        &self.module_registry
    }

    // ── Discovery & Filtering ──

    /// Search plugins by keyword (matches name, description, keywords).
    pub fn search(&self, query: &str) -> Vec<&PluginDescriptor> {
        let q = query.to_lowercase();
        self.plugins.values()
            .map(|p| &p.descriptor)
            .filter(|d| {
                d.name.to_lowercase().contains(&q)
                    || d.description.to_lowercase().contains(&q)
                    || d.keywords.iter().any(|k| k.to_lowercase().contains(&q))
            })
            .collect()
    }

    /// Filter plugins by category.
    pub fn by_category(&self, category: &PluginCategory) -> Vec<&PluginDescriptor> {
        self.plugins.values()
            .map(|p| &p.descriptor)
            .filter(|d| d.category == *category)
            .collect()
    }

    /// Filter plugins by platform support.
    pub fn by_platform(&self, target: &BuildTarget) -> Vec<&PluginDescriptor> {
        self.plugins.values()
            .map(|p| &p.descriptor)
            .filter(|d| d.supports_platform(target))
            .collect()
    }

    /// Filter plugins by capability.
    pub fn by_capability(&self, cap: &PluginCapability) -> Vec<&PluginDescriptor> {
        self.plugins.values()
            .map(|p| &p.descriptor)
            .filter(|d| d.capabilities.contains(cap))
            .collect()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{MethodDescriptor, ModuleArg, ModuleResult, ModuleValue};

    // ── Test module ──
    struct TestModule { name: String }
    impl NativeModule for TestModule {
        fn name(&self) -> &str { &self.name }
        fn methods(&self) -> Vec<MethodDescriptor> {
            vec![MethodDescriptor::sync("ping", "returns pong")]
        }
        fn invoke_sync(&self, method: &str, _args: &[ModuleArg]) -> ModuleResult {
            match method {
                "ping" => Ok(ModuleValue::String("pong".into())),
                _ => Err(ModuleError::MethodNotFound {
                    module: self.name.clone(), method: method.into(),
                }),
            }
        }
    }

    fn test_descriptor(name: &str) -> PluginDescriptor {
        PluginDescriptor {
            name: name.into(),
            version: PluginVersion::new(1, 0, 0),
            description: format!("{name} plugin"),
            author: "test".into(),
            license: "MIT".into(),
            homepage: None,
            repository: None,
            category: PluginCategory::Platform,
            capabilities: vec![PluginCapability::NativeModule],
            supported_platforms: vec![BuildTarget::Ios, BuildTarget::Android, BuildTarget::Web],
            min_engine_version: PluginVersion::new(0, 1, 0),
            dependencies: vec![],
            js_package: None,
            keywords: vec!["test".into()],
        }
    }

    // ── Plugin Spec Tests ──

    #[test]
    fn test_plugin_version_comparison() {
        let v1 = PluginVersion::new(1, 0, 0);
        let v1_1 = PluginVersion::new(1, 1, 0);
        let v2 = PluginVersion::new(2, 0, 0);

        assert!(v1 < v1_1);
        assert!(v1_1 < v2);
        assert!(v1.satisfies_min(&v1));
        assert!(v1_1.satisfies_min(&v1));
        assert!(!v1.satisfies_min(&v2));
    }

    #[test]
    fn test_plugin_version_display() {
        assert_eq!(PluginVersion::new(3, 2, 1).to_string(), "3.2.1");
    }

    #[test]
    fn test_descriptor_platform_support() {
        let desc = test_descriptor("Camera");
        assert!(desc.supports_platform(&BuildTarget::Ios));
        assert!(!desc.supports_platform(&BuildTarget::Linux));
    }

    #[test]
    fn test_descriptor_engine_compatibility() {
        let desc = test_descriptor("Test");
        assert!(desc.is_compatible_with_engine(&PluginVersion::new(1, 0, 0)));
        assert!(desc.is_compatible_with_engine(&PluginVersion::new(0, 1, 0)));
        assert!(!desc.is_compatible_with_engine(&PluginVersion::new(0, 0, 9)));
    }

    #[test]
    fn test_descriptor_dependency_check() {
        let mut desc = test_descriptor("Dep");
        desc.dependencies.push(PluginDependency {
            name: "Core".into(),
            min_version: PluginVersion::new(1, 0, 0),
        });

        let mut installed = HashMap::new();
        assert_eq!(desc.check_dependencies(&installed), vec!["Core"]);

        installed.insert("Core".into(), PluginVersion::new(1, 0, 0));
        assert!(desc.check_dependencies(&installed).is_empty());
    }

    #[test]
    fn test_validate_descriptor_ok() {
        let desc = test_descriptor("Valid");
        assert!(validate_descriptor(&desc).is_ok());
    }

    #[test]
    fn test_validate_descriptor_empty_name() {
        let mut desc = test_descriptor("");
        desc.name = String::new();
        assert!(validate_descriptor(&desc).is_err());
    }

    #[test]
    fn test_validate_descriptor_no_platforms() {
        let mut desc = test_descriptor("NoPlatform");
        desc.supported_platforms.clear();
        assert!(validate_descriptor(&desc).is_err());
    }

    // ── Plugin Registry Tests ──

    #[test]
    fn test_install_plugin_without_module() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        let desc = test_descriptor("Analytics");
        assert!(registry.install(desc, None).is_ok());
        assert!(registry.is_installed("Analytics"));
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_install_plugin_with_module() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        let desc = test_descriptor("Camera");
        let module = Arc::new(TestModule { name: "Camera".into() }) as Arc<dyn NativeModule>;
        assert!(registry.install(desc, Some(module)).is_ok());
        assert!(registry.module_registry().has("Camera"));
    }

    #[test]
    fn test_install_duplicate_rejected() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        assert!(registry.install(test_descriptor("Dup"), None).is_ok());
        assert!(registry.install(test_descriptor("Dup"), None).is_err());
    }

    #[test]
    fn test_install_incompatible_engine_rejected() {
        let mut registry = PluginRegistry::new(PluginVersion::new(0, 0, 1));
        let mut desc = test_descriptor("New");
        desc.min_engine_version = PluginVersion::new(2, 0, 0);
        assert!(registry.install(desc, None).is_err());
    }

    #[test]
    fn test_install_missing_dependency_rejected() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        let mut desc = test_descriptor("Child");
        desc.dependencies.push(PluginDependency {
            name: "Parent".into(),
            min_version: PluginVersion::new(1, 0, 0),
        });
        assert!(registry.install(desc, None).is_err());
    }

    #[test]
    fn test_install_with_satisfied_dependency() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        assert!(registry.install(test_descriptor("Parent"), None).is_ok());

        let mut child = test_descriptor("Child");
        child.dependencies.push(PluginDependency {
            name: "Parent".into(),
            min_version: PluginVersion::new(1, 0, 0),
        });
        assert!(registry.install(child, None).is_ok());
    }

    #[test]
    fn test_uninstall_plugin() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        registry.install(test_descriptor("Removable"), None).unwrap();
        assert!(registry.uninstall("Removable").is_ok());
        assert!(!registry.is_installed("Removable"));
    }

    #[test]
    fn test_uninstall_depended_on_fails() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        registry.install(test_descriptor("Base"), None).unwrap();

        let mut child = test_descriptor("Extension");
        child.dependencies.push(PluginDependency {
            name: "Base".into(),
            min_version: PluginVersion::new(1, 0, 0),
        });
        registry.install(child, None).unwrap();

        // Can't uninstall Base — Extension depends on it
        assert!(registry.uninstall("Base").is_err());
        assert!(registry.is_installed("Base"));
    }

    #[test]
    fn test_enable_disable() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        registry.install(test_descriptor("Toggle"), None).unwrap();

        assert_eq!(registry.enabled_plugins().len(), 1);
        registry.set_enabled("Toggle", false).unwrap();
        assert_eq!(registry.enabled_plugins().len(), 0);
        registry.set_enabled("Toggle", true).unwrap();
        assert_eq!(registry.enabled_plugins().len(), 1);
    }

    // ── Discovery Tests ──

    #[test]
    fn test_search_by_name() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        registry.install(test_descriptor("Camera"), None).unwrap();
        registry.install(test_descriptor("Storage"), None).unwrap();
        registry.install(test_descriptor("CameraRoll"), None).unwrap();

        let results = registry.search("camera");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_keyword() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));
        let mut desc = test_descriptor("Tracker");
        desc.keywords.push("analytics".into());
        registry.install(desc, None).unwrap();

        let results = registry.search("analytics");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Tracker");
    }

    #[test]
    fn test_filter_by_category() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));

        let mut ui_plugin = test_descriptor("Button");
        ui_plugin.category = PluginCategory::UI;
        registry.install(ui_plugin, None).unwrap();

        let mut data_plugin = test_descriptor("SQLite");
        data_plugin.category = PluginCategory::Data;
        registry.install(data_plugin, None).unwrap();

        assert_eq!(registry.by_category(&PluginCategory::UI).len(), 1);
        assert_eq!(registry.by_category(&PluginCategory::Data).len(), 1);
        assert_eq!(registry.by_category(&PluginCategory::Auth).len(), 0);
    }

    #[test]
    fn test_filter_by_platform() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));

        let mut ios_only = test_descriptor("ARKit");
        ios_only.supported_platforms = vec![BuildTarget::Ios];
        registry.install(ios_only, None).unwrap();

        registry.install(test_descriptor("Universal"), None).unwrap();

        assert_eq!(registry.by_platform(&BuildTarget::Ios).len(), 2);
        assert_eq!(registry.by_platform(&BuildTarget::Android).len(), 1); // Universal only
    }

    #[test]
    fn test_filter_by_capability() {
        let mut registry = PluginRegistry::new(PluginVersion::new(1, 0, 0));

        let mut theme_plugin = test_descriptor("DarkMode");
        theme_plugin.capabilities = vec![PluginCapability::Theme];
        registry.install(theme_plugin, None).unwrap();

        registry.install(test_descriptor("Module"), None).unwrap();

        assert_eq!(registry.by_capability(&PluginCapability::Theme).len(), 1);
        assert_eq!(registry.by_capability(&PluginCapability::NativeModule).len(), 1);
    }

    // ── Serialization ──

    #[test]
    fn test_descriptor_serialization() {
        let desc = test_descriptor("SerTest");
        let json = serde_json::to_string(&desc).unwrap();
        let restored: PluginDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "SerTest");
        assert_eq!(restored.version, PluginVersion::new(1, 0, 0));
        assert_eq!(restored.supported_platforms.len(), 3);
    }
}
