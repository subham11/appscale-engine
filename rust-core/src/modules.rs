//! Native Module System
//!
//! Provides a registry for native modules that can be called from JavaScript
//! through the bridge layer. Modules register methods that can be invoked
//! either synchronously or asynchronously.
//!
//! Design:
//! - Modules implement the `NativeModule` trait
//! - The `ModuleRegistry` holds all registered modules
//! - Methods are invoked by name through the bridge
//! - Thread safety: registry is `Send + Sync`, modules must be `Send + Sync`
//!
//! Example:
//! ```ignore
//! struct StorageModule;
//! impl NativeModule for StorageModule {
//!     fn name(&self) -> &str { "Storage" }
//!     fn methods(&self) -> Vec<MethodDescriptor> {
//!         vec![
//!             MethodDescriptor::sync("getItem", "Get a stored value"),
//!             MethodDescriptor::async_method("setItem", "Store a value"),
//!         ]
//!     }
//!     fn invoke_sync(&self, method: &str, args: &[ModuleArg]) -> ModuleResult { ... }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Module argument and result types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Arguments passed to module method invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModuleArg {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<ModuleArg>),
    Map(HashMap<String, ModuleArg>),
}

/// Result from a module method invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModuleValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<ModuleValue>),
    Map(HashMap<String, ModuleValue>),
}

/// Result type for module method calls.
pub type ModuleResult = Result<ModuleValue, ModuleError>;

/// Errors from module operations.
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    #[error("Method not found: {module}.{method}")]
    MethodNotFound { module: String, method: String },

    #[error("Invalid arguments: {0}")]
    InvalidArgs(String),

    #[error("Method {0} is async, use invoke_async instead")]
    IsAsync(String),

    #[error("Module error: {0}")]
    Internal(String),
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Method descriptor
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Describes a method exposed by a native module.
#[derive(Debug, Clone)]
pub struct MethodDescriptor {
    pub name: String,
    pub description: String,
    pub is_async: bool,
}

impl MethodDescriptor {
    pub fn sync(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            is_async: false,
        }
    }

    pub fn async_method(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            is_async: true,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Native module trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Trait that all native modules must implement.
///
/// Modules must be `Send + Sync` to be safely shared across threads.
pub trait NativeModule: Send + Sync {
    /// Unique name of the module (e.g., "Storage", "Camera", "Haptics").
    fn name(&self) -> &str;

    /// List of methods this module exposes.
    fn methods(&self) -> Vec<MethodDescriptor>;

    /// Invoke a synchronous method. Must not block for long.
    fn invoke_sync(&self, method: &str, _args: &[ModuleArg]) -> ModuleResult {
        Err(ModuleError::MethodNotFound {
            module: self.name().to_string(),
            method: method.to_string(),
        })
    }

    /// Invoke an asynchronous method. Returns a boxed future.
    /// Default implementation returns MethodNotFound.
    fn invoke_async(
        &self,
        method: &str,
        _args: &[ModuleArg],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ModuleResult> + Send + '_>> {
        let module = self.name().to_string();
        let method = method.to_string();
        Box::pin(async move { Err(ModuleError::MethodNotFound { module, method }) })
    }

    /// Called when the module is registered. Use for initialization.
    fn on_init(&self) {}

    /// Called when the module is being torn down.
    fn on_destroy(&self) {}
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Module registry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Thread-safe registry of native modules.
pub struct ModuleRegistry {
    modules: RwLock<HashMap<String, Arc<dyn NativeModule>>>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            modules: RwLock::new(HashMap::new()),
        }
    }

    /// Register a native module. Returns error if a module with the same name exists.
    pub fn register(&self, module: Arc<dyn NativeModule>) -> Result<(), ModuleError> {
        let name = module.name().to_string();
        let mut modules = self.modules.write().unwrap();
        if modules.contains_key(&name) {
            return Err(ModuleError::Internal(format!(
                "Module '{name}' already registered"
            )));
        }
        module.on_init();
        modules.insert(name, module);
        Ok(())
    }

    /// Unregister a module by name.
    pub fn unregister(&self, name: &str) -> Option<Arc<dyn NativeModule>> {
        let mut modules = self.modules.write().unwrap();
        let module = modules.remove(name);
        if let Some(ref m) = module {
            m.on_destroy();
        }
        module
    }

    /// Check if a module is registered.
    pub fn has(&self, name: &str) -> bool {
        self.modules.read().unwrap().contains_key(name)
    }

    /// Get a list of all registered module names.
    pub fn module_names(&self) -> Vec<String> {
        self.modules.read().unwrap().keys().cloned().collect()
    }

    /// Get method descriptors for a module.
    pub fn module_methods(&self, name: &str) -> Result<Vec<MethodDescriptor>, ModuleError> {
        let modules = self.modules.read().unwrap();
        let module = modules
            .get(name)
            .ok_or_else(|| ModuleError::ModuleNotFound(name.to_string()))?;
        Ok(module.methods())
    }

    /// Invoke a synchronous method on a module.
    pub fn invoke_sync(&self, module_name: &str, method: &str, args: &[ModuleArg]) -> ModuleResult {
        let modules = self.modules.read().unwrap();
        let module = modules
            .get(module_name)
            .ok_or_else(|| ModuleError::ModuleNotFound(module_name.to_string()))?;

        // Verify method exists and is sync
        let methods = module.methods();
        let desc = methods.iter().find(|m| m.name == method);
        match desc {
            None => Err(ModuleError::MethodNotFound {
                module: module_name.to_string(),
                method: method.to_string(),
            }),
            Some(d) if d.is_async => Err(ModuleError::IsAsync(method.to_string())),
            Some(_) => module.invoke_sync(method, args),
        }
    }

    /// Invoke an async method on a module.
    pub async fn invoke_async(
        &self,
        module_name: &str,
        method: &str,
        args: &[ModuleArg],
    ) -> ModuleResult {
        let module = {
            let modules = self.modules.read().unwrap();
            modules
                .get(module_name)
                .ok_or_else(|| ModuleError::ModuleNotFound(module_name.to_string()))?
                .clone()
        };

        module.invoke_async(method, args).await
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestModule;

    impl NativeModule for TestModule {
        fn name(&self) -> &str {
            "Test"
        }

        fn methods(&self) -> Vec<MethodDescriptor> {
            vec![
                MethodDescriptor::sync("greet", "Returns a greeting"),
                MethodDescriptor::sync("add", "Adds two numbers"),
                MethodDescriptor::async_method("fetch", "Fetches data"),
            ]
        }

        fn invoke_sync(&self, method: &str, args: &[ModuleArg]) -> ModuleResult {
            match method {
                "greet" => {
                    let name = match args.first() {
                        Some(ModuleArg::String(s)) => s.as_str(),
                        _ => "World",
                    };
                    Ok(ModuleValue::String(format!("Hello, {name}!")))
                }
                "add" => {
                    let a = match args.first() {
                        Some(ModuleArg::Int(n)) => *n,
                        _ => return Err(ModuleError::InvalidArgs("expected int".into())),
                    };
                    let b = match args.get(1) {
                        Some(ModuleArg::Int(n)) => *n,
                        _ => return Err(ModuleError::InvalidArgs("expected int".into())),
                    };
                    Ok(ModuleValue::Int(a + b))
                }
                _ => Err(ModuleError::MethodNotFound {
                    module: self.name().to_string(),
                    method: method.to_string(),
                }),
            }
        }
    }

    #[test]
    fn test_register_and_invoke() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();

        assert!(registry.has("Test"));
        assert!(!registry.has("Unknown"));

        let result = registry
            .invoke_sync("Test", "greet", &[ModuleArg::String("Rust".into())])
            .unwrap();
        assert!(matches!(result, ModuleValue::String(s) if s == "Hello, Rust!"));
    }

    #[test]
    fn test_add_method() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();

        let result = registry
            .invoke_sync("Test", "add", &[ModuleArg::Int(3), ModuleArg::Int(4)])
            .unwrap();
        assert!(matches!(result, ModuleValue::Int(7)));
    }

    #[test]
    fn test_module_not_found() {
        let registry = ModuleRegistry::new();
        let result = registry.invoke_sync("Missing", "greet", &[]);
        assert!(matches!(result, Err(ModuleError::ModuleNotFound(_))));
    }

    #[test]
    fn test_method_not_found() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        let result = registry.invoke_sync("Test", "missing", &[]);
        assert!(matches!(result, Err(ModuleError::MethodNotFound { .. })));
    }

    #[test]
    fn test_async_method_guard() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        let result = registry.invoke_sync("Test", "fetch", &[]);
        assert!(matches!(result, Err(ModuleError::IsAsync(_))));
    }

    #[test]
    fn test_duplicate_register() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        let result = registry.register(Arc::new(TestModule));
        assert!(result.is_err());
    }

    #[test]
    fn test_unregister() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        assert!(registry.has("Test"));

        let removed = registry.unregister("Test");
        assert!(removed.is_some());
        assert!(!registry.has("Test"));
    }

    #[test]
    fn test_module_names() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        let names = registry.module_names();
        assert_eq!(names, vec!["Test".to_string()]);
    }

    #[test]
    fn test_module_methods_list() {
        let registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule)).unwrap();
        let methods = registry.module_methods("Test").unwrap();
        assert_eq!(methods.len(), 3);
        assert_eq!(methods[0].name, "greet");
        assert!(!methods[0].is_async);
        assert_eq!(methods[2].name, "fetch");
        assert!(methods[2].is_async);
    }
}
