//! `appscale dev` — Development server with hot reload.
//!
//! Starts a development server that:
//! 1. Watches source files for changes
//! 2. Bundles TypeScript/JSX via Metro-like bundler
//! 3. Sends hot updates to the running app via WebSocket
//! 4. Runs the Rust core engine in debug mode

use anyhow::Result;

pub fn run(port: u16, platform: &str) -> Result<()> {
    println!("AppScale Development Server");
    println!("  Platform: {platform}");
    println!("  Port:     {port}");
    println!("  URL:      http://localhost:{port}");
    println!();

    validate_platform(platform)?;

    println!("Starting dev server...");
    println!("  [1/3] Compiling Rust core (debug)...");
    println!("  [2/3] Bundling TypeScript sources...");
    println!("  [3/3] Starting hot reload server on :{port}...");
    println!();
    println!("  Ready! Watching for changes...");

    // TODO: Phase 2 — integrate with actual bundler + file watcher
    // - Use `notify` crate for file watching
    // - Spawn Metro or custom bundler subprocess
    // - WebSocket server for hot module replacement
    // - `cargo build` subprocess for Rust changes

    println!("\n  (Dev server scaffold — full implementation in Phase 2)");

    Ok(())
}

fn validate_platform(platform: &str) -> Result<()> {
    match platform {
        "ios" | "android" | "web" | "macos" | "windows" => Ok(()),
        _ => anyhow::bail!(
            "Unknown platform '{}'. Supported: ios, android, web, macos, windows",
            platform
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_platforms() {
        assert!(validate_platform("ios").is_ok());
        assert!(validate_platform("android").is_ok());
        assert!(validate_platform("web").is_ok());
        assert!(validate_platform("macos").is_ok());
        assert!(validate_platform("windows").is_ok());
    }

    #[test]
    fn invalid_platform() {
        assert!(validate_platform("linux").is_err());
        assert!(validate_platform("").is_err());
    }
}
