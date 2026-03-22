//! `appscale create <project>` — Project scaffolding.
//!
//! Generates a new AppScale project with:
//! - React app template (App.tsx, index.tsx)
//! - Rust core setup (Cargo.toml referencing appscale-core)
//! - Platform configs (Xcode project stub, Gradle stub, webpack config)
//! - package.json with @appscale/core dependency

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn run(name: &str, template: &str) -> Result<()> {
    let project_dir = Path::new(name);

    if project_dir.exists() {
        anyhow::bail!("Directory '{}' already exists", name);
    }

    println!("Creating AppScale project: {name}");
    println!("  Template: {template}");

    // Create directory structure
    let dirs = [
        "",
        "src",
        "src/components",
        "rust",
        "platforms/ios",
        "platforms/android",
        "platforms/web",
    ];

    for dir in &dirs {
        let path = project_dir.join(dir);
        fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }

    // package.json
    let package_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "private": true,
        "scripts": {
            "dev": "appscale dev",
            "build:ios": "appscale build ios",
            "build:android": "appscale build android",
            "build:web": "appscale build web",
            "build:macos": "appscale build macos",
            "build:windows": "appscale build windows"
        },
        "dependencies": {
            "react": "^18.3.0",
            "react-reconciler": "^0.30.0",
            "@appscale/core": "^0.1.0"
        },
        "devDependencies": {
            "typescript": "^5.4.0",
            "@types/react": "^18.3.0"
        }
    });
    write_file(
        &project_dir.join("package.json"),
        &serde_json::to_string_pretty(&package_json)?,
    )?;

    // tsconfig.json
    let tsconfig = serde_json::json!({
        "compilerOptions": {
            "target": "ES2020",
            "module": "ESNext",
            "moduleResolution": "bundler",
            "jsx": "react-jsx",
            "strict": true,
            "esModuleInterop": true,
            "outDir": "dist",
            "rootDir": "src",
            "declaration": true,
            "sourceMap": true
        },
        "include": ["src"],
        "exclude": ["node_modules", "dist"]
    });
    write_file(
        &project_dir.join("tsconfig.json"),
        &serde_json::to_string_pretty(&tsconfig)?,
    )?;

    // App.tsx
    write_file(
        &project_dir.join("src/App.tsx"),
        &r#"import React from 'react';

export default function App() {
  return (
    <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center' }}>
      <Text style={{ fontSize: 24, fontWeight: 'bold' }}>
        Welcome to {APP_NAME}
      </Text>
      <Text style={{ fontSize: 16, color: '#666', marginTop: 8 }}>
        Built with AppScale Engine
      </Text>
    </View>
  );
}
"#
        .replace("{APP_NAME}", name),
    )?;

    // index.tsx
    write_file(
        &project_dir.join("src/index.tsx"),
        r#"import { AppRegistry } from '@appscale/core';
import App from './App';

AppRegistry.registerComponent('App', () => App);
"#,
    )?;

    // Rust Cargo.toml for native modules
    let cargo_toml = format!(
        r#"[package]
name = "{name}-native"
version = "0.1.0"
edition = "2021"

[dependencies]
appscale-core = {{ path = "../../rust-core" }}
"#
    );
    write_file(&project_dir.join("rust/Cargo.toml"), &cargo_toml)?;

    // Rust lib.rs stub
    write_file(
        &project_dir.join("rust/src/lib.rs"),
        r#"//! Native modules for this AppScale project.
//! Add custom Rust modules here that will be callable from React.

pub fn init() {
    // Register custom native modules here
}
"#,
    )?;
    fs::create_dir_all(project_dir.join("rust/src")).context("Failed to create rust/src")?;

    // .gitignore
    write_file(
        &project_dir.join(".gitignore"),
        "node_modules/\ndist/\ntarget/\n*.lock\n.DS_Store\n",
    )?;

    // README
    write_file(
        &project_dir.join("README.md"),
        &format!(
            r#"# {name}

An AppScale cross-platform app.

## Getting Started

```bash
npm install
appscale dev          # Start dev server
appscale build web    # Build for web
appscale build ios    # Build for iOS
appscale build android  # Build for Android
```
"#
        ),
    )?;

    println!("\n  Project created at ./{name}");
    println!("\n  Next steps:");
    println!("    cd {name}");
    println!("    npm install");
    println!("    appscale dev");

    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write: {}", path.display()))
}
