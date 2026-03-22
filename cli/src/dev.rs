//! `appscale dev` — Development server with hot reload.
//!
//! Starts a development server that:
//! 1. Compiles the Rust native code (debug mode)
//! 2. Watches source files for changes and recompiles
//! 3. For web: serves the output on a local HTTP server
//! 4. For native: rebuilds and relaunches the app

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub fn run(port: u16, platform: &str) -> Result<()> {
    validate_platform(platform)?;

    let project_root = env::current_dir().context("Cannot determine current directory")?;
    let rust_dir = project_root.join("rust");

    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found. Run `appscale create` first.");
    }

    println!("AppScale Development Server");
    println!("  Platform: {platform}");
    println!("  Port:     {port}");
    println!();

    // Step 1: initial Rust build
    println!("  [1/3] Compiling Rust core (debug)...");
    let status = Command::new("cargo")
        .arg("build")
        .current_dir(&rust_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("Initial Rust compilation failed");
    }
    println!("    ✓ Compilation succeeded");

    match platform {
        "web" => dev_web(&project_root, &rust_dir, port),
        "macos" => dev_native(&project_root, &rust_dir, "macos"),
        other => {
            println!("  [2/3] Building {other} in development mode...");

            // Do a build and report
            let status = Command::new("cargo")
                .arg("build")
                .current_dir(&rust_dir)
                .status()?;
            if status.success() {
                println!("    ✓ Build succeeded");
            }

            println!("  [3/3] Watching for changes...");
            watch_and_rebuild(&project_root, &rust_dir)?;
            Ok(())
        }
    }
}

/// Web dev mode: compile WASM, serve files, watch for changes.
fn dev_web(project_root: &Path, rust_dir: &Path, port: u16) -> Result<()> {
    // Check for wasm target
    let has_wasm = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
        .unwrap_or(false);
    if !has_wasm {
        println!("    Installing wasm32-unknown-unknown target...");
        let _ = Command::new("rustup")
            .args(["target", "add", "wasm32-unknown-unknown"])
            .status();
    }

    // Build WASM
    println!("  [2/3] Compiling Rust → WASM...");
    let status = Command::new("cargo")
        .args(["build", "--target", "wasm32-unknown-unknown"])
        .current_dir(rust_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("WASM compilation failed");
    }
    println!("    ✓ WASM compiled");

    // Copy wasm to a serve directory
    let serve_dir = project_root.join(".appscale-dev");
    fs::create_dir_all(&serve_dir)?;

    let wasm_dir = rust_dir.join("target/wasm32-unknown-unknown/debug");
    if let Ok(entries) = fs::read_dir(&wasm_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "wasm") {
                fs::copy(entry.path(), serve_dir.join("engine.wasm"))?;
                break;
            }
        }
    }

    // Generate dev index.html
    let name = project_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "AppScale".into());

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>{name} — Dev</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: system-ui, sans-serif; display: flex;
               justify-content: center; align-items: center;
               min-height: 100vh; background: #fafafa; }}
        .container {{ text-align: center; }}
        h1 {{ font-size: 2rem; margin-bottom: 0.5rem; }}
        .sub {{ color: #666; margin-bottom: 1rem; }}
        .status {{ padding: 0.4rem 1rem; background: #e8f5e9;
                   color: #2e7d32; border-radius: 4px; display: inline-block; }}
        .dev {{ margin-top: 1rem; color: #999; font-size: 0.8rem; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>Welcome to {name}</h1>
        <p class="sub">Built with AppScale Engine</p>
        <span class="status" id="s">Loading…</span>
        <p class="dev">Dev server • port {port}</p>
    </div>
    <script>
        fetch('engine.wasm').then(r => {{
            if (r.ok) return r.arrayBuffer();
            throw new Error('not found');
        }}).then(b => {{
            document.getElementById('s').textContent =
                'WASM engine loaded ✓ (' + Math.round(b.byteLength/1024) + ' KB)';
        }}).catch(() => {{
            const s = document.getElementById('s');
            s.textContent = 'engine.wasm not found';
            s.style.background = '#fff3e0';
            s.style.color = '#e65100';
        }});
    </script>
</body>
</html>"#,
        name = name,
        port = port,
    );
    fs::write(serve_dir.join("index.html"), html)?;

    // Start simple HTTP server
    println!("  [3/3] Starting HTTP server on :{port}...");
    println!();
    println!("  ✓ Ready! Open http://localhost:{port}");
    println!("  Press Ctrl+C to stop");
    println!();

    serve_directory(&serve_dir, port)?;

    Ok(())
}

/// Native dev mode: build and run as macOS app, watch for rebuilds.
fn dev_native(project_root: &Path, rust_dir: &Path, _platform: &str) -> Result<()> {
    println!("  [2/3] Building macOS app (debug)...");

    // Use the build module to do a full macOS build
    let status = Command::new("cargo")
        .arg("build")
        .current_dir(rust_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("Rust compilation failed");
    }
    println!("    ✓ Build succeeded");

    println!("  [3/3] Watching for changes...");
    println!();
    println!("  ✓ Ready! Watching for source changes...");
    println!("  Press Ctrl+C to stop");
    println!();

    watch_and_rebuild(project_root, rust_dir)?;
    Ok(())
}

/// Simple file watcher: polls src/ and rust/src/ for changes, triggers
/// cargo build on modification.
fn watch_and_rebuild(project_root: &Path, rust_dir: &Path) -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(r);

    let watch_dirs = vec![project_root.join("src"), rust_dir.join("src")];

    let mut last_check = SystemTime::now();

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_secs(2));

        let mut changed = false;
        for dir in &watch_dirs {
            if dir.exists() && has_changes_since(dir, last_check) {
                changed = true;
                break;
            }
        }

        if changed {
            last_check = SystemTime::now();
            println!("  ↻ Change detected, rebuilding...");
            let status = Command::new("cargo")
                .arg("build")
                .current_dir(rust_dir)
                .status();
            match status {
                Ok(s) if s.success() => println!("  ✓ Rebuild succeeded"),
                Ok(_) => println!("  ✗ Build failed — fix errors and save again"),
                Err(e) => println!("  ✗ Could not run cargo: {e}"),
            }
        }
    }

    println!("\n  Stopped.");
    Ok(())
}

fn has_changes_since(dir: &Path, since: SystemTime) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if has_changes_since(&path, since) {
                return true;
            }
        } else if let Ok(meta) = path.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified > since {
                    return true;
                }
            }
        }
    }
    false
}

/// Minimal HTTP file server.
fn serve_directory(dir: &Path, port: u16) -> Result<()> {
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .with_context(|| format!("Cannot bind to port {port}"))?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(r);

    // Non-blocking so we can check the stop flag
    listener.set_nonblocking(true)?;

    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _addr)) => {
                let mut buf = [0u8; 4096];
                let _ = stream.set_nonblocking(false);
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);

                // Parse GET path
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");

                // Sanitize: no directory traversal
                let clean = path.trim_start_matches('/');
                let clean = if clean.is_empty() {
                    "index.html"
                } else {
                    clean
                };

                // Reject path traversal
                if clean.contains("..") {
                    let resp = "HTTP/1.1 403 Forbidden\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                    continue;
                }

                let file_path = dir.join(clean);
                if file_path.exists() && file_path.is_file() {
                    let content_type = match file_path.extension().and_then(|e| e.to_str()) {
                        Some("html") => "text/html; charset=utf-8",
                        Some("js") => "application/javascript",
                        Some("wasm") => "application/wasm",
                        Some("css") => "text/css",
                        Some("json") => "application/json",
                        _ => "application/octet-stream",
                    };
                    if let Ok(body) = fs::read(&file_path) {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                } else {
                    let resp = "HTTP/1.1 404 Not Found\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }

    println!("\n  Server stopped.");
    Ok(())
}

fn ctrlc_handler(flag: Arc<AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        flag.store(false, Ordering::SeqCst);
    });
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
