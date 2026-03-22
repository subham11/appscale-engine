//! Storage Abstraction — Unified key-value, secure, and file storage API.
//!
//! Provides a trait-based storage layer that abstracts across platforms:
//! - Key-value store (AsyncStorage equivalent)
//! - Secure storage (iOS Keychain, Android Keystore, Web crypto)
//! - File system access (sandboxed app data)
//!
//! Platform adapters implement `StorageBackend` for each target.
//! The `StorageManager` coordinates across backends and provides
//! a type-safe, thread-safe API for the rest of the engine.
//!
//! Design:
//! - All operations are synchronous in the trait (platform can spawn internally)
//! - `StorageBackend` is `Send + Sync` for safe sharing across threads
//! - Secure storage uses a separate trait method set with explicit domain tagging

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Error types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Errors from storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Storage backend not available: {0}")]
    BackendUnavailable(String),

    #[error("Secure storage not supported on this platform")]
    SecureNotSupported,

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Storage quota exceeded")]
    QuotaExceeded,

    #[error("Storage I/O error: {0}")]
    IoError(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Storage value types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Values that can be stored in key-value storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum StorageValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<StorageValue>),
    Map(HashMap<String, StorageValue>),
}

impl StorageValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            StorageValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            StorageValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            StorageValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            StorageValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, StorageValue::Null)
    }
}

impl From<String> for StorageValue {
    fn from(s: String) -> Self {
        StorageValue::String(s)
    }
}

impl From<&str> for StorageValue {
    fn from(s: &str) -> Self {
        StorageValue::String(s.to_string())
    }
}

impl From<bool> for StorageValue {
    fn from(b: bool) -> Self {
        StorageValue::Bool(b)
    }
}

impl From<i64> for StorageValue {
    fn from(i: i64) -> Self {
        StorageValue::Int(i)
    }
}

impl From<f64> for StorageValue {
    fn from(f: f64) -> Self {
        StorageValue::Float(f)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Storage namespace
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Namespace for isolating storage domains.
/// Prevents key collisions between different parts of the app.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageNamespace(String);

impl StorageNamespace {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Default namespace for app data.
    pub fn app_data() -> Self {
        Self("app_data".to_string())
    }

    /// Namespace for user preferences.
    pub fn preferences() -> Self {
        Self("preferences".to_string())
    }

    /// Namespace for cache data (can be cleared by system).
    pub fn cache() -> Self {
        Self("cache".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Build a namespaced key: "namespace:key"
    pub fn prefixed_key(&self, key: &str) -> String {
        format!("{}:{}", self.0, key)
    }
}

impl Default for StorageNamespace {
    fn default() -> Self {
        Self::app_data()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Storage backend trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Trait that platform-specific storage implementations must implement.
///
/// Covers three storage tiers:
/// 1. Key-value store (general app data)
/// 2. Secure storage (credentials, tokens)
/// 3. File storage (documents, media)
pub trait StorageBackend: Send + Sync {
    /// Platform identifier for this backend.
    fn platform_id(&self) -> &str;

    // ── Key-Value Store ──────────────────────────────────

    /// Get a value by key within a namespace.
    fn get(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>>;

    /// Set a value by key within a namespace.
    fn set(
        &self,
        namespace: &StorageNamespace,
        key: &str,
        value: StorageValue,
    ) -> StorageResult<()>;

    /// Delete a key within a namespace.
    fn delete(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<()>;

    /// Check if a key exists within a namespace.
    fn contains(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<bool> {
        Ok(self.get(namespace, key)?.is_some())
    }

    /// List all keys in a namespace.
    fn keys(&self, namespace: &StorageNamespace) -> StorageResult<Vec<String>>;

    /// Clear all keys in a namespace.
    fn clear(&self, namespace: &StorageNamespace) -> StorageResult<()>;

    // ── Secure Storage ─────────────────────────────────

    /// Whether this platform supports secure storage (Keychain, Keystore, etc.).
    fn supports_secure(&self) -> bool {
        false
    }

    /// Store a value securely (e.g., iOS Keychain, Android Keystore).
    fn secure_set(&self, _key: &str, _value: &[u8]) -> StorageResult<()> {
        Err(StorageError::SecureNotSupported)
    }

    /// Retrieve a securely stored value.
    fn secure_get(&self, _key: &str) -> StorageResult<Option<Vec<u8>>> {
        Err(StorageError::SecureNotSupported)
    }

    /// Delete a securely stored value.
    fn secure_delete(&self, _key: &str) -> StorageResult<()> {
        Err(StorageError::SecureNotSupported)
    }

    // ── File Storage ───────────────────────────────────

    /// Write bytes to a file path (relative to app sandbox).
    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()>;

    /// Read bytes from a file path (relative to app sandbox).
    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>>;

    /// Delete a file.
    fn delete_file(&self, path: &str) -> StorageResult<()>;

    /// Check if a file exists.
    fn file_exists(&self, path: &str) -> StorageResult<bool>;

    /// List files in a directory (relative to app sandbox).
    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Storage manager
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// High-level storage coordinator.
/// Wraps a `StorageBackend` and provides convenience methods.
pub struct StorageManager {
    backend: Arc<dyn StorageBackend>,
}

impl StorageManager {
    pub fn new(backend: Arc<dyn StorageBackend>) -> Self {
        Self { backend }
    }

    pub fn platform_id(&self) -> &str {
        self.backend.platform_id()
    }

    // ── Key-Value convenience methods ────────────────────

    /// Get a string value.
    pub fn get_string(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<String>> {
        match self.backend.get(ns, key)? {
            Some(StorageValue::String(s)) => Ok(Some(s)),
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }

    /// Set a string value.
    pub fn set_string(&self, ns: &StorageNamespace, key: &str, value: &str) -> StorageResult<()> {
        self.backend
            .set(ns, key, StorageValue::String(value.to_string()))
    }

    /// Get a boolean value.
    pub fn get_bool(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<bool>> {
        match self.backend.get(ns, key)? {
            Some(StorageValue::Bool(b)) => Ok(Some(b)),
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }

    /// Get a value, deserializing from JSON stored as a string.
    pub fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        ns: &StorageNamespace,
        key: &str,
    ) -> StorageResult<Option<T>> {
        match self.backend.get(ns, key)? {
            Some(StorageValue::String(s)) => {
                let val = serde_json::from_str(&s)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(val))
            }
            _ => Ok(None),
        }
    }

    /// Set a value, serializing to JSON and storing as a string.
    pub fn set_json<T: Serialize>(
        &self,
        ns: &StorageNamespace,
        key: &str,
        value: &T,
    ) -> StorageResult<()> {
        let json = serde_json::to_string(value)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        self.backend.set(ns, key, StorageValue::String(json))
    }

    /// Multi-get: fetch multiple keys at once.
    pub fn multi_get(
        &self,
        ns: &StorageNamespace,
        keys: &[&str],
    ) -> StorageResult<HashMap<String, StorageValue>> {
        let mut result = HashMap::new();
        for &key in keys {
            if let Some(val) = self.backend.get(ns, key)? {
                result.insert(key.to_string(), val);
            }
        }
        Ok(result)
    }

    /// Multi-set: store multiple key-value pairs at once.
    pub fn multi_set(
        &self,
        ns: &StorageNamespace,
        entries: &[(&str, StorageValue)],
    ) -> StorageResult<()> {
        for (key, value) in entries {
            self.backend.set(ns, key, value.clone())?;
        }
        Ok(())
    }

    // ── Secure storage convenience ───────────────────────

    pub fn supports_secure(&self) -> bool {
        self.backend.supports_secure()
    }

    /// Store a string securely.
    pub fn secure_set_string(&self, key: &str, value: &str) -> StorageResult<()> {
        self.backend.secure_set(key, value.as_bytes())
    }

    /// Retrieve a securely stored string.
    pub fn secure_get_string(&self, key: &str) -> StorageResult<Option<String>> {
        match self.backend.secure_get(key)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    // ── File storage convenience ─────────────────────────

    /// Write a string to a file.
    pub fn write_text(&self, path: &str, text: &str) -> StorageResult<()> {
        self.backend.write_file(path, text.as_bytes())
    }

    /// Read a file as a UTF-8 string.
    pub fn read_text(&self, path: &str) -> StorageResult<String> {
        let bytes = self.backend.read_file(path)?;
        String::from_utf8(bytes).map_err(|e| StorageError::SerializationError(e.to_string()))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// In-memory backend (testing + Web fallback)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// In-memory storage backend for testing and as a Web fallback.
/// All data is lost when the process exits.
pub struct MemoryStorageBackend {
    platform: String,
    kv: RwLock<HashMap<String, StorageValue>>,
    secure: RwLock<HashMap<String, Vec<u8>>>,
    files: RwLock<HashMap<String, Vec<u8>>>,
}

impl MemoryStorageBackend {
    pub fn new(platform: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            kv: RwLock::new(HashMap::new()),
            secure: RwLock::new(HashMap::new()),
            files: RwLock::new(HashMap::new()),
        }
    }
}

impl StorageBackend for MemoryStorageBackend {
    fn platform_id(&self) -> &str {
        &self.platform
    }

    fn get(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>> {
        let full_key = namespace.prefixed_key(key);
        let store = self.kv.read().unwrap();
        Ok(store.get(&full_key).cloned())
    }

    fn set(
        &self,
        namespace: &StorageNamespace,
        key: &str,
        value: StorageValue,
    ) -> StorageResult<()> {
        let full_key = namespace.prefixed_key(key);
        let mut store = self.kv.write().unwrap();
        store.insert(full_key, value);
        Ok(())
    }

    fn delete(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<()> {
        let full_key = namespace.prefixed_key(key);
        let mut store = self.kv.write().unwrap();
        store.remove(&full_key);
        Ok(())
    }

    fn contains(&self, namespace: &StorageNamespace, key: &str) -> StorageResult<bool> {
        let full_key = namespace.prefixed_key(key);
        let store = self.kv.read().unwrap();
        Ok(store.contains_key(&full_key))
    }

    fn keys(&self, namespace: &StorageNamespace) -> StorageResult<Vec<String>> {
        let prefix = format!("{}:", namespace.as_str());
        let store = self.kv.read().unwrap();
        let keys: Vec<String> = store
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .map(|k| k[prefix.len()..].to_string())
            .collect();
        Ok(keys)
    }

    fn clear(&self, namespace: &StorageNamespace) -> StorageResult<()> {
        let prefix = format!("{}:", namespace.as_str());
        let mut store = self.kv.write().unwrap();
        store.retain(|k, _| !k.starts_with(&prefix));
        Ok(())
    }

    fn supports_secure(&self) -> bool {
        true
    }

    fn secure_set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        let mut secure = self.secure.write().unwrap();
        secure.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn secure_get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        let secure = self.secure.read().unwrap();
        Ok(secure.get(key).cloned())
    }

    fn secure_delete(&self, key: &str) -> StorageResult<()> {
        let mut secure = self.secure.write().unwrap();
        secure.remove(key);
        Ok(())
    }

    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        let mut files = self.files.write().unwrap();
        files.insert(path.to_string(), data.to_vec());
        Ok(())
    }

    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>> {
        let files = self.files.read().unwrap();
        files
            .get(path)
            .cloned()
            .ok_or_else(|| StorageError::FileNotFound(path.to_string()))
    }

    fn delete_file(&self, path: &str) -> StorageResult<()> {
        let mut files = self.files.write().unwrap();
        files.remove(path);
        Ok(())
    }

    fn file_exists(&self, path: &str) -> StorageResult<bool> {
        let files = self.files.read().unwrap();
        Ok(files.contains_key(path))
    }

    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>> {
        let prefix = if dir.ends_with('/') {
            dir.to_string()
        } else {
            format!("{dir}/")
        };
        let files = self.files.read().unwrap();
        let listing: Vec<String> = files
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect();
        Ok(listing)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Platform adapter stubs
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// iOS storage backend stub.
/// Production: NSUserDefaults (KV) + Keychain (secure) + FileManager (files).
pub struct IosStorageBackend {
    memory: MemoryStorageBackend,
}

impl Default for IosStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl IosStorageBackend {
    pub fn new() -> Self {
        Self {
            memory: MemoryStorageBackend::new("ios"),
        }
    }
}

impl StorageBackend for IosStorageBackend {
    fn platform_id(&self) -> &str {
        "ios"
    }
    fn get(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>> {
        self.memory.get(ns, key)
    }
    fn set(&self, ns: &StorageNamespace, key: &str, value: StorageValue) -> StorageResult<()> {
        self.memory.set(ns, key, value)
    }
    fn delete(&self, ns: &StorageNamespace, key: &str) -> StorageResult<()> {
        self.memory.delete(ns, key)
    }
    fn keys(&self, ns: &StorageNamespace) -> StorageResult<Vec<String>> {
        self.memory.keys(ns)
    }
    fn clear(&self, ns: &StorageNamespace) -> StorageResult<()> {
        self.memory.clear(ns)
    }
    fn supports_secure(&self) -> bool {
        true
    } // iOS Keychain
    fn secure_set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.memory.secure_set(key, value)
    }
    fn secure_get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        self.memory.secure_get(key)
    }
    fn secure_delete(&self, key: &str) -> StorageResult<()> {
        self.memory.secure_delete(key)
    }
    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.memory.write_file(path, data)
    }
    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>> {
        self.memory.read_file(path)
    }
    fn delete_file(&self, path: &str) -> StorageResult<()> {
        self.memory.delete_file(path)
    }
    fn file_exists(&self, path: &str) -> StorageResult<bool> {
        self.memory.file_exists(path)
    }
    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>> {
        self.memory.list_files(dir)
    }
}

/// Android storage backend stub.
/// Production: SharedPreferences (KV) + Android Keystore (secure) + app files dir (files).
pub struct AndroidStorageBackend {
    memory: MemoryStorageBackend,
}

impl Default for AndroidStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidStorageBackend {
    pub fn new() -> Self {
        Self {
            memory: MemoryStorageBackend::new("android"),
        }
    }
}

impl StorageBackend for AndroidStorageBackend {
    fn platform_id(&self) -> &str {
        "android"
    }
    fn get(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>> {
        self.memory.get(ns, key)
    }
    fn set(&self, ns: &StorageNamespace, key: &str, value: StorageValue) -> StorageResult<()> {
        self.memory.set(ns, key, value)
    }
    fn delete(&self, ns: &StorageNamespace, key: &str) -> StorageResult<()> {
        self.memory.delete(ns, key)
    }
    fn keys(&self, ns: &StorageNamespace) -> StorageResult<Vec<String>> {
        self.memory.keys(ns)
    }
    fn clear(&self, ns: &StorageNamespace) -> StorageResult<()> {
        self.memory.clear(ns)
    }
    fn supports_secure(&self) -> bool {
        true
    } // Android Keystore
    fn secure_set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.memory.secure_set(key, value)
    }
    fn secure_get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        self.memory.secure_get(key)
    }
    fn secure_delete(&self, key: &str) -> StorageResult<()> {
        self.memory.secure_delete(key)
    }
    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.memory.write_file(path, data)
    }
    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>> {
        self.memory.read_file(path)
    }
    fn delete_file(&self, path: &str) -> StorageResult<()> {
        self.memory.delete_file(path)
    }
    fn file_exists(&self, path: &str) -> StorageResult<bool> {
        self.memory.file_exists(path)
    }
    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>> {
        self.memory.list_files(dir)
    }
}

/// Web storage backend stub.
/// Production: localStorage (KV) + SubtleCrypto + encrypted localStorage (secure) + IndexedDB (files).
pub struct WebStorageBackend {
    memory: MemoryStorageBackend,
}

impl Default for WebStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WebStorageBackend {
    pub fn new() -> Self {
        Self {
            memory: MemoryStorageBackend::new("web"),
        }
    }
}

impl StorageBackend for WebStorageBackend {
    fn platform_id(&self) -> &str {
        "web"
    }
    fn get(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>> {
        self.memory.get(ns, key)
    }
    fn set(&self, ns: &StorageNamespace, key: &str, value: StorageValue) -> StorageResult<()> {
        self.memory.set(ns, key, value)
    }
    fn delete(&self, ns: &StorageNamespace, key: &str) -> StorageResult<()> {
        self.memory.delete(ns, key)
    }
    fn keys(&self, ns: &StorageNamespace) -> StorageResult<Vec<String>> {
        self.memory.keys(ns)
    }
    fn clear(&self, ns: &StorageNamespace) -> StorageResult<()> {
        self.memory.clear(ns)
    }
    fn supports_secure(&self) -> bool {
        false
    } // Web has limited secure storage
    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.memory.write_file(path, data)
    }
    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>> {
        self.memory.read_file(path)
    }
    fn delete_file(&self, path: &str) -> StorageResult<()> {
        self.memory.delete_file(path)
    }
    fn file_exists(&self, path: &str) -> StorageResult<bool> {
        self.memory.file_exists(path)
    }
    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>> {
        self.memory.list_files(dir)
    }
}

/// Desktop storage backend stub (macOS, Windows, Linux).
/// Production: file-based KV (JSON files) + OS keyring (secure) + file system (files).
pub struct DesktopStorageBackend {
    memory: MemoryStorageBackend,
}

impl Default for DesktopStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopStorageBackend {
    pub fn new() -> Self {
        Self {
            memory: MemoryStorageBackend::new("desktop"),
        }
    }
}

impl StorageBackend for DesktopStorageBackend {
    fn platform_id(&self) -> &str {
        "desktop"
    }
    fn get(&self, ns: &StorageNamespace, key: &str) -> StorageResult<Option<StorageValue>> {
        self.memory.get(ns, key)
    }
    fn set(&self, ns: &StorageNamespace, key: &str, value: StorageValue) -> StorageResult<()> {
        self.memory.set(ns, key, value)
    }
    fn delete(&self, ns: &StorageNamespace, key: &str) -> StorageResult<()> {
        self.memory.delete(ns, key)
    }
    fn keys(&self, ns: &StorageNamespace) -> StorageResult<Vec<String>> {
        self.memory.keys(ns)
    }
    fn clear(&self, ns: &StorageNamespace) -> StorageResult<()> {
        self.memory.clear(ns)
    }
    fn supports_secure(&self) -> bool {
        true
    } // OS keyring
    fn secure_set(&self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.memory.secure_set(key, value)
    }
    fn secure_get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        self.memory.secure_get(key)
    }
    fn secure_delete(&self, key: &str) -> StorageResult<()> {
        self.memory.secure_delete(key)
    }
    fn write_file(&self, path: &str, data: &[u8]) -> StorageResult<()> {
        self.memory.write_file(path, data)
    }
    fn read_file(&self, path: &str) -> StorageResult<Vec<u8>> {
        self.memory.read_file(path)
    }
    fn delete_file(&self, path: &str) -> StorageResult<()> {
        self.memory.delete_file(path)
    }
    fn file_exists(&self, path: &str) -> StorageResult<bool> {
        self.memory.file_exists(path)
    }
    fn list_files(&self, dir: &str) -> StorageResult<Vec<String>> {
        self.memory.list_files(dir)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> StorageManager {
        StorageManager::new(Arc::new(MemoryStorageBackend::new("test")))
    }

    #[test]
    fn kv_set_get_delete() {
        let mgr = make_manager();
        let ns = StorageNamespace::app_data();

        // Initially empty
        assert!(mgr.get_string(&ns, "name").unwrap().is_none());

        // Set and get
        mgr.set_string(&ns, "name", "AppScale").unwrap();
        assert_eq!(mgr.get_string(&ns, "name").unwrap().unwrap(), "AppScale");

        // Delete
        mgr.backend.delete(&ns, "name").unwrap();
        assert!(mgr.get_string(&ns, "name").unwrap().is_none());
    }

    #[test]
    fn kv_namespaces_isolated() {
        let mgr = make_manager();
        let ns1 = StorageNamespace::app_data();
        let ns2 = StorageNamespace::preferences();

        mgr.set_string(&ns1, "key", "value1").unwrap();
        mgr.set_string(&ns2, "key", "value2").unwrap();

        assert_eq!(mgr.get_string(&ns1, "key").unwrap().unwrap(), "value1");
        assert_eq!(mgr.get_string(&ns2, "key").unwrap().unwrap(), "value2");
    }

    #[test]
    fn kv_clear_namespace() {
        let mgr = make_manager();
        let ns = StorageNamespace::cache();

        mgr.set_string(&ns, "a", "1").unwrap();
        mgr.set_string(&ns, "b", "2").unwrap();
        mgr.backend.clear(&ns).unwrap();

        assert!(mgr.get_string(&ns, "a").unwrap().is_none());
        assert!(mgr.get_string(&ns, "b").unwrap().is_none());
    }

    #[test]
    fn kv_list_keys() {
        let mgr = make_manager();
        let ns = StorageNamespace::app_data();

        mgr.set_string(&ns, "alpha", "1").unwrap();
        mgr.set_string(&ns, "beta", "2").unwrap();
        mgr.set_string(&ns, "gamma", "3").unwrap();

        let mut keys = mgr.backend.keys(&ns).unwrap();
        keys.sort();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn kv_contains() {
        let mgr = make_manager();
        let ns = StorageNamespace::app_data();

        assert!(!mgr.backend.contains(&ns, "missing").unwrap());
        mgr.set_string(&ns, "present", "here").unwrap();
        assert!(mgr.backend.contains(&ns, "present").unwrap());
    }

    #[test]
    fn kv_json_roundtrip() {
        let mgr = make_manager();
        let ns = StorageNamespace::app_data();

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Config {
            debug: bool,
            max_items: u32,
        }

        let config = Config {
            debug: true,
            max_items: 50,
        };
        mgr.set_json(&ns, "config", &config).unwrap();

        let loaded: Config = mgr.get_json(&ns, "config").unwrap().unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn multi_get_set() {
        let mgr = make_manager();
        let ns = StorageNamespace::app_data();

        mgr.multi_set(
            &ns,
            &[
                ("x", StorageValue::Int(1)),
                ("y", StorageValue::Int(2)),
                ("z", StorageValue::Int(3)),
            ],
        )
        .unwrap();

        let result = mgr.multi_get(&ns, &["x", "z", "missing"]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result["x"], StorageValue::Int(1));
        assert_eq!(result["z"], StorageValue::Int(3));
    }

    #[test]
    fn secure_storage() {
        let mgr = make_manager();
        assert!(mgr.supports_secure());

        mgr.secure_set_string("api_token", "secret123").unwrap();
        let token = mgr.secure_get_string("api_token").unwrap().unwrap();
        assert_eq!(token, "secret123");

        mgr.backend.secure_delete("api_token").unwrap();
        assert!(mgr.secure_get_string("api_token").unwrap().is_none());
    }

    #[test]
    fn file_operations() {
        let mgr = make_manager();

        assert!(!mgr.backend.file_exists("docs/readme.txt").unwrap());

        mgr.write_text("docs/readme.txt", "Hello, AppScale!")
            .unwrap();
        assert!(mgr.backend.file_exists("docs/readme.txt").unwrap());

        let text = mgr.read_text("docs/readme.txt").unwrap();
        assert_eq!(text, "Hello, AppScale!");

        let files = mgr.backend.list_files("docs").unwrap();
        assert_eq!(files, vec!["docs/readme.txt"]);

        mgr.backend.delete_file("docs/readme.txt").unwrap();
        assert!(!mgr.backend.file_exists("docs/readme.txt").unwrap());
    }

    #[test]
    fn file_not_found() {
        let mgr = make_manager();
        let result = mgr.read_text("nonexistent.txt");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), StorageError::FileNotFound(_)));
    }

    #[test]
    fn storage_value_conversions() {
        let v: StorageValue = "hello".into();
        assert_eq!(v.as_str(), Some("hello"));

        let v: StorageValue = true.into();
        assert_eq!(v.as_bool(), Some(true));

        let v: StorageValue = 42i64.into();
        assert_eq!(v.as_i64(), Some(42));

        let v: StorageValue = std::f64::consts::PI.into();
        assert_eq!(v.as_f64(), Some(std::f64::consts::PI));

        assert!(StorageValue::Null.is_null());
    }

    #[test]
    fn platform_adapters_construct() {
        let ios = IosStorageBackend::new();
        assert_eq!(ios.platform_id(), "ios");
        assert!(ios.supports_secure());

        let android = AndroidStorageBackend::new();
        assert_eq!(android.platform_id(), "android");
        assert!(android.supports_secure());

        let web = WebStorageBackend::new();
        assert_eq!(web.platform_id(), "web");
        assert!(!web.supports_secure());

        let desktop = DesktopStorageBackend::new();
        assert_eq!(desktop.platform_id(), "desktop");
        assert!(desktop.supports_secure());
    }

    #[test]
    fn namespace_prefixed_keys() {
        let ns = StorageNamespace::new("myapp");
        assert_eq!(ns.prefixed_key("user"), "myapp:user");
        assert_eq!(ns.as_str(), "myapp");
    }
}
