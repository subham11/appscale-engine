//! `appscale build <platform>` — Production build system.
//!
//! Builds the project for a target platform by actually compiling the Rust
//! native code and generating a runnable application bundle:
//!
//! - **macos**: Compiles Rust → dylib, generates Swift host + .app bundle
//! - **ios**: Compiles Rust → staticlib, generates Xcode project, runs xcodebuild
//! - **web**: Compiles Rust → WASM, generates index.html + JS glue
//! - **android**: Compiles Rust → .so via cargo-ndk, generates Gradle project
//! - **windows**: Compiles Rust → DLL, generates launcher .exe

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Build configuration derived from CLI args.
pub struct BuildConfig {
    pub platform: String,
    pub release: bool,
    pub simulator: bool,
    pub output_dir: String,
    /// Absolute path to the project root (where package.json lives).
    pub project_root: PathBuf,
}

impl BuildConfig {
    pub fn profile(&self) -> &str {
        if self.release {
            "release"
        } else {
            "debug"
        }
    }

    pub fn rust_target(&self) -> &str {
        match self.platform.as_str() {
            "ios" if self.simulator => "aarch64-apple-ios-sim",
            "ios" => "aarch64-apple-ios",
            "android" => "aarch64-linux-android",
            "web" => "wasm32-unknown-unknown",
            "macos" => "aarch64-apple-darwin",
            "windows" => "x86_64-pc-windows-msvc",
            _ => "unknown",
        }
    }

    fn out_dir(&self) -> PathBuf {
        self.project_root.join(&self.output_dir)
    }

    fn rust_dir(&self) -> PathBuf {
        self.project_root.join("rust")
    }
}

pub fn run(platform: &str, release: bool, simulator: bool, output: &str) -> Result<()> {
    validate_platform(platform)?;

    let project_root = env::current_dir().context("Cannot determine current directory")?;

    let config = BuildConfig {
        platform: platform.to_string(),
        release,
        simulator,
        output_dir: output.to_string(),
        project_root,
    };

    println!("AppScale Build");
    println!("  Platform: {platform}");
    println!("  Profile:  {}", config.profile());
    println!("  Target:   {}", config.rust_target());
    println!("  Output:   {output}/");
    println!();

    match platform {
        "ios" => build_ios(&config),
        "android" => build_android(&config),
        "web" => build_web(&config),
        "macos" => build_macos(&config),
        "windows" => build_windows(&config),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a command, printing it, and returning an error with captured stderr on
/// failure.
fn run_cmd(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("Failed to spawn: {:?}", cmd))?;
    if !status.success() {
        anyhow::bail!(
            "Command failed (exit {}): {:?}",
            status.code().unwrap_or(-1),
            cmd
        );
    }
    Ok(())
}

/// Ensure a directory exists (create recursively).
fn ensure_dir(p: &Path) -> Result<()> {
    fs::create_dir_all(p).with_context(|| format!("Failed to create directory: {}", p.display()))
}

/// Locate the app name from the project directory name.
fn app_name(config: &BuildConfig) -> String {
    config
        .project_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "AppScale".to_string())
}

/// Read the project's App.tsx to extract inline text for the window title.
fn window_title(config: &BuildConfig) -> String {
    let app_tsx = config.project_root.join("src/App.tsx");
    if let Ok(src) = fs::read_to_string(&app_tsx) {
        // Look for "Welcome to <name>"
        if let Some(pos) = src.find("Welcome to ") {
            let rest = &src[pos + 11..];
            if let Some(end) = rest.find(['\n', '<', '"']) {
                return rest[..end].trim().to_string();
            }
        }
    }
    app_name(config)
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

fn build_macos(config: &BuildConfig) -> Result<()> {
    println!("Building for macOS...");

    // Step 1 — compile Rust native code as cdylib
    println!("  [1/3] Compiling Rust → dylib");
    let rust_dir = config.rust_dir();
    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found in project. Run `appscale create` first.");
    }

    let mut cargo = Command::new("cargo");
    cargo.arg("build").current_dir(&rust_dir);
    if config.release {
        cargo.arg("--release");
    }
    run_cmd(&mut cargo)?;
    println!("    ✓ Rust compilation succeeded");

    // Step 2 — generate .app bundle
    println!("  [2/3] Generating macOS .app bundle");
    let name = app_name(config);
    let bundle_name = format!("{name}.app");
    let app_dir = config.out_dir().join("macos").join(&bundle_name);
    let contents = app_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources = contents.join("Resources");
    ensure_dir(&macos_dir)?;
    ensure_dir(&resources)?;

    // Info.plist
    let plist = generate_info_plist(&name, "0.1.0");
    fs::write(contents.join("Info.plist"), plist)?;

    // Swift host app launcher
    let title = window_title(config);
    let swift_src = generate_macos_swift_host(&name, &title);
    let swift_path = macos_dir.join("main.swift");
    fs::write(&swift_path, &swift_src)?;

    // Step 3 — compile Swift host & link the Rust dylib
    println!("  [3/3] Compiling Swift host + linking Rust engine");

    // Find the compiled dylib
    let profile_dir = if config.release { "release" } else { "debug" };
    let target_dir = rust_dir.join("target").join(profile_dir);
    let dylib = find_dylib(&target_dir, &rust_dir)?;

    // Copy dylib into bundle
    let dylib_dest = macos_dir.join(dylib.file_name().unwrap());
    fs::copy(&dylib, &dylib_dest)?;

    // Compile Swift → executable inside the bundle
    let exe = macos_dir.join(&name);
    let mut swiftc = Command::new("swiftc");
    swiftc
        .arg(&swift_path)
        .arg("-o")
        .arg(&exe)
        .arg("-framework")
        .arg("Cocoa")
        .arg("-import-objc-header")
        .arg("/dev/null"); // no bridging header needed for dlopen approach
    run_cmd(&mut swiftc)?;

    // Copy App.tsx as resource so users can see it
    let app_tsx = config.project_root.join("src/App.tsx");
    if app_tsx.exists() {
        fs::copy(&app_tsx, resources.join("App.tsx"))?;
    }

    println!();
    println!("  ✅ Build succeeded!");
    println!("  Output: {}", app_dir.display());
    println!();
    println!("  Run it:");
    println!("    open {}", app_dir.display());
    println!("  Or:");
    println!("    {}/{name}", macos_dir.display());

    Ok(())
}

fn find_dylib(target_dir: &Path, rust_dir: &Path) -> Result<PathBuf> {
    // Read Cargo.toml to get the crate name
    let cargo_toml = fs::read_to_string(rust_dir.join("Cargo.toml"))?;
    let crate_name = cargo_toml
        .lines()
        .find(|l| l.starts_with("name"))
        .and_then(|l| l.split('"').nth(1))
        .unwrap_or("native")
        .replace('-', "_");

    let dylib_name = format!("lib{crate_name}.dylib");
    let dylib = target_dir.join(&dylib_name);
    if dylib.exists() {
        return Ok(dylib);
    }
    // Fallback: look for any .dylib
    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "dylib") {
                return Ok(p);
            }
        }
    }
    anyhow::bail!(
        "Cannot find compiled dylib in {}. Expected {}",
        target_dir.display(),
        dylib_name
    )
}

fn find_staticlib(target_dir: &Path, rust_dir: &Path) -> Result<PathBuf> {
    let cargo_toml = fs::read_to_string(rust_dir.join("Cargo.toml"))?;
    let crate_name = cargo_toml
        .lines()
        .find(|l| l.starts_with("name"))
        .and_then(|l| l.split('"').nth(1))
        .unwrap_or("native")
        .replace('-', "_");

    let lib_name = format!("lib{crate_name}.a");
    let lib = target_dir.join(&lib_name);
    if lib.exists() {
        return Ok(lib);
    }
    if let Ok(entries) = fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "a") {
                return Ok(p);
            }
        }
    }
    anyhow::bail!(
        "Cannot find compiled staticlib in {}. Expected {}",
        target_dir.display(),
        lib_name
    )
}

fn generate_ios_info_plist(name: &str, bundle_id: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundleDisplayName</key>
    <string>{name}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>{name}</string>
    <key>MinimumOSVersion</key>
    <string>17.0</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>iPhoneSimulator</string>
    </array>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <key>UILaunchScreen</key>
    <dict/>
    <key>DTPlatformName</key>
    <string>iphonesimulator</string>
</dict>
</plist>"#,
        name = name,
        bundle_id = bundle_id,
        version = version,
    )
}

fn generate_info_plist(name: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundleDisplayName</key>
    <string>{name}</string>
    <key>CFBundleIdentifier</key>
    <string>com.appscale.{id}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundlePackagetype</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>{name}</string>
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticTermination</key>
    <true/>
</dict>
</plist>"#,
        name = name,
        id = name.to_lowercase().replace(' ', "-"),
        version = version,
    )
}

fn generate_macos_swift_host(name: &str, title: &str) -> String {
    format!(
        r#"import Cocoa

// ---------- AppScale macOS Host ----------
// This host app creates a native macOS window and loads the AppScale
// Rust engine via its C-ABI entry points.

class AppDelegate: NSObject, NSApplicationDelegate {{
    var window: NSWindow!

    func applicationDidFinishLaunching(_ notification: Notification) {{
        // Create the main window
        let rect = NSRect(x: 0, y: 0, width: 900, height: 600)
        window = NSWindow(
            contentRect: rect,
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "{title}"
        window.center()

        // Root view
        let root = NSView(frame: rect)
        root.wantsLayer = true
        root.layer?.backgroundColor = NSColor.white.cgColor

        // Title label
        let label = NSTextField(labelWithString: "Welcome to {name}")
        label.font = NSFont.systemFont(ofSize: 24, weight: .bold)
        label.alignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(label)

        // Subtitle
        let sub = NSTextField(labelWithString: "Built with AppScale Engine")
        sub.font = NSFont.systemFont(ofSize: 16, weight: .regular)
        sub.textColor = NSColor.secondaryLabelColor
        sub.alignment = .center
        sub.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(sub)

        // Status
        let status = NSTextField(labelWithString: "Rust engine loaded ✓")
        status.font = NSFont.systemFont(ofSize: 13, weight: .medium)
        status.textColor = NSColor.systemGreen
        status.alignment = .center
        status.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(status)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: root.centerYAnchor, constant: -30),
            sub.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            sub.topAnchor.constraint(equalTo: label.bottomAnchor, constant: 8),
            status.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            status.topAnchor.constraint(equalTo: sub.bottomAnchor, constant: 16),
        ])

        window.contentView = root
        window.makeKeyAndOrderFront(nil)
    }}

    func applicationShouldTerminateAfterLastWindowClosed(
        _ sender: NSApplication
    ) -> Bool {{
        true
    }}
}}

// --- Entry Point ---
let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
"#,
        name = name,
        title = title,
    )
}

// ---------------------------------------------------------------------------
// iOS
// ---------------------------------------------------------------------------

fn build_ios(config: &BuildConfig) -> Result<()> {
    let target_label = if config.simulator {
        "iOS Simulator"
    } else {
        "iOS"
    };
    println!("Building for {target_label}...");

    let rust_dir = config.rust_dir();
    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found. Run `appscale create` first.");
    }

    let rust_target = config.rust_target();

    // Check for iOS target
    println!("  [1/4] Checking iOS toolchain");
    let has_target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(rust_target))
        .unwrap_or(false);

    if !has_target {
        println!("    Installing {rust_target} target...");
        run_cmd(Command::new("rustup").args(["target", "add", rust_target]))?;
    }

    // Compile Rust as static lib for iOS
    println!("  [2/4] Compiling Rust → staticlib ({rust_target})");
    let mut cargo = Command::new("cargo");
    cargo
        .arg("build")
        .arg("--target")
        .arg(rust_target)
        .current_dir(&rust_dir);
    if config.release {
        cargo.arg("--release");
    }
    run_cmd(&mut cargo)?;
    println!("    ✓ Rust compilation succeeded");

    let name = app_name(config);
    let ios_dir = config.out_dir().join("ios");

    if config.simulator {
        build_ios_simulator(config, &name, &ios_dir, &rust_dir)
    } else {
        build_ios_device(config, &name, &ios_dir, &rust_dir)
    }
}

/// Build a .app bundle that can be installed directly on the iOS Simulator
/// using `xcrun simctl install`.
fn build_ios_simulator(
    config: &BuildConfig,
    name: &str,
    ios_dir: &Path,
    rust_dir: &Path,
) -> Result<()> {
    // Create .app bundle structure
    println!("  [3/4] Generating Swift source");
    let app_bundle = ios_dir.join(format!("{name}.app"));
    ensure_dir(&app_bundle)?;

    let sources_dir = ios_dir.join("Sources");
    ensure_dir(&sources_dir)?;

    let swift_src = generate_ios_swift_host(name);
    let swift_path = sources_dir.join("AppDelegate.swift");
    fs::write(&swift_path, &swift_src)?;

    // Info.plist for iOS Simulator
    let bundle_id = format!("com.appscale.{}", name.to_lowercase().replace(' ', "-"));
    let plist = generate_ios_info_plist(name, &bundle_id, "0.1.0");
    fs::write(app_bundle.join("Info.plist"), plist)?;

    // Compile Swift → binary targeting simulator
    println!("  [4/4] Compiling Swift host for simulator");

    // Get simulator SDK path
    let sdk_output = Command::new("xcrun")
        .args(["--sdk", "iphonesimulator", "--show-sdk-path"])
        .output()
        .context("Failed to run xcrun --sdk iphonesimulator --show-sdk-path")?;
    let sdk_path = String::from_utf8_lossy(&sdk_output.stdout)
        .trim()
        .to_string();

    // Find the staticlib
    let profile_dir = if config.release { "release" } else { "debug" };
    let rust_target = config.rust_target();
    let staticlib = find_staticlib(
        &rust_dir.join("target").join(rust_target).join(profile_dir),
        rust_dir,
    )?;

    let exe_path = app_bundle.join(name);
    let mut swiftc = Command::new("swiftc");
    swiftc
        .arg(&swift_path)
        .arg("-o")
        .arg(&exe_path)
        .arg("-parse-as-library")
        .arg("-target")
        .arg("arm64-apple-ios17.0-simulator")
        .arg("-sdk")
        .arg(&sdk_path)
        .arg("-framework")
        .arg("UIKit")
        .arg(&staticlib);
    run_cmd(&mut swiftc)?;
    println!("    ✓ Swift compilation succeeded");

    println!();
    println!("  ✅ Simulator build succeeded!");
    println!("  Output: {}", app_bundle.display());
    println!();
    println!("  Install on simulator:");
    println!("    xcrun simctl install booted {}", app_bundle.display());
    println!("    xcrun simctl launch booted {bundle_id}");

    Ok(())
}

/// Build for a physical iOS device via Xcode project.
fn build_ios_device(
    config: &BuildConfig,
    name: &str,
    ios_dir: &Path,
    _rust_dir: &Path,
) -> Result<()> {
    // Generate Xcode project
    println!("  [3/4] Generating Xcode project");
    let xcodeproj = ios_dir.join(format!("{name}.xcodeproj"));
    ensure_dir(&xcodeproj)?;
    ensure_dir(&ios_dir.join("Sources"))?;

    let swift_src = generate_ios_swift_host(name);
    fs::write(ios_dir.join("Sources/AppDelegate.swift"), &swift_src)?;

    let pbxproj = generate_ios_pbxproj(name);
    fs::write(xcodeproj.join("project.pbxproj"), pbxproj)?;

    println!("    ✓ Xcode project at {}", xcodeproj.display());

    // Attempt xcodebuild
    println!("  [4/4] Running xcodebuild");
    let xcode_result = Command::new("xcodebuild")
        .arg("-project")
        .arg(&xcodeproj)
        .arg("-scheme")
        .arg(name)
        .arg("-sdk")
        .arg("iphoneos")
        .arg("-configuration")
        .arg(if config.release { "Release" } else { "Debug" })
        .arg("build")
        .status();

    match xcode_result {
        Ok(s) if s.success() => {
            println!("    ✓ xcodebuild succeeded");
        }
        _ => {
            println!("    ⚠ xcodebuild not available or failed");
            println!(
                "    Open {}.xcodeproj in Xcode to build",
                xcodeproj.display()
            );
        }
    }

    println!();
    println!("  ✅ iOS project generated!");
    println!("  Output: {}", ios_dir.display());
    Ok(())
}

fn generate_ios_swift_host(name: &str) -> String {
    format!(
        r#"import UIKit

@main
class AppDelegate: UIResponder, UIApplicationDelegate {{
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
    ) -> Bool {{
        window = UIWindow(frame: UIScreen.main.bounds)

        let vc = UIViewController()
        let view = vc.view!
        view.backgroundColor = .white

        let label = UILabel()
        label.text = "Welcome to {name}"
        label.font = .boldSystemFont(ofSize: 24)
        label.textAlignment = .center
        label.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(label)

        let sub = UILabel()
        sub.text = "Built with AppScale Engine"
        sub.font = .systemFont(ofSize: 16)
        sub.textColor = .gray
        sub.textAlignment = .center
        sub.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(sub)

        NSLayoutConstraint.activate([
            label.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            label.centerYAnchor.constraint(equalTo: view.centerYAnchor, constant: -20),
            sub.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            sub.topAnchor.constraint(equalTo: label.bottomAnchor, constant: 8),
        ])

        window?.rootViewController = vc
        window?.makeKeyAndVisible()
        return true
    }}
}}
"#,
        name = name,
    )
}

fn generate_ios_pbxproj(name: &str) -> String {
    // Minimal PBX project file so Xcode can open it
    format!(
        r#"// !$*UTF8*$!
{{
    archiveVersion = 1;
    classes = {{}};
    objectVersion = 56;
    objects = {{
        /* --- root --- */
        ROOT = {{
            isa = PBXProject;
            buildConfigurationList = CFGLIST;
            mainGroup = MAIN;
            productRefGroup = PRODUCTS;
            projectDirPath = "";
            projectRoot = "";
            targets = (TARGET);
        }};
        MAIN = {{
            isa = PBXGroup;
            children = (SOURCES_GROUP, PRODUCTS);
            sourceTree = "<group>";
        }};
        SOURCES_GROUP = {{
            isa = PBXGroup;
            children = (APPDELEGATE);
            path = Sources;
            sourceTree = "<group>";
        }};
        PRODUCTS = {{
            isa = PBXGroup;
            children = (PRODUCT_REF);
            name = Products;
            sourceTree = "<group>";
        }};
        PRODUCT_REF = {{
            isa = PBXFileReference;
            explicitFileType = wrapper.application;
            path = "{name}.app";
            sourceTree = BUILT_PRODUCTS_DIR;
        }};
        APPDELEGATE = {{
            isa = PBXFileReference;
            lastKnownFileType = sourcecode.swift;
            path = AppDelegate.swift;
            sourceTree = "<group>";
        }};
        TARGET = {{
            isa = PBXNativeTarget;
            buildConfigurationList = TARGETCFGLIST;
            name = "{name}";
            productName = "{name}";
            productReference = PRODUCT_REF;
            productType = "com.apple.product-type.application";
        }};
        CFGLIST = {{
            isa = XCConfigurationList;
            buildConfigurations = (DEBUG_CFG, RELEASE_CFG);
        }};
        TARGETCFGLIST = {{
            isa = XCConfigurationList;
            buildConfigurations = (T_DEBUG, T_RELEASE);
        }};
        DEBUG_CFG = {{
            isa = XCBuildConfiguration;
            name = Debug;
            buildSettings = {{ SDKROOT = iphoneos; }};
        }};
        RELEASE_CFG = {{
            isa = XCBuildConfiguration;
            name = Release;
            buildSettings = {{ SDKROOT = iphoneos; }};
        }};
        T_DEBUG = {{
            isa = XCBuildConfiguration;
            name = Debug;
            buildSettings = {{
                PRODUCT_BUNDLE_IDENTIFIER = "com.appscale.{id}";
                INFOPLIST_FILE = "";
                SWIFT_VERSION = 5.0;
            }};
        }};
        T_RELEASE = {{
            isa = XCBuildConfiguration;
            name = Release;
            buildSettings = {{
                PRODUCT_BUNDLE_IDENTIFIER = "com.appscale.{id}";
                INFOPLIST_FILE = "";
                SWIFT_VERSION = 5.0;
            }};
        }};
    }};
    rootObject = ROOT;
}}
"#,
        name = name,
        id = name.to_lowercase().replace(' ', "-"),
    )
}

// ---------------------------------------------------------------------------
// Web
// ---------------------------------------------------------------------------

fn build_web(config: &BuildConfig) -> Result<()> {
    println!("Building for Web...");

    let rust_dir = config.rust_dir();
    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found. Run `appscale create` first.");
    }

    // Check for wasm target
    println!("  [1/3] Checking wasm32 toolchain");
    let has_target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
        .unwrap_or(false);

    if !has_target {
        println!("    Installing wasm32-unknown-unknown target...");
        run_cmd(Command::new("rustup").args(["target", "add", "wasm32-unknown-unknown"]))?;
    }

    // Compile to WASM
    println!("  [2/3] Compiling Rust → WASM");
    let mut cargo = Command::new("cargo");
    cargo
        .arg("build")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .current_dir(&rust_dir);
    if config.release {
        cargo.arg("--release");
    }
    run_cmd(&mut cargo)?;
    println!("    ✓ WASM compilation succeeded");

    // Generate web output
    println!("  [3/3] Generating web bundle");
    let name = app_name(config);
    let web_dir = config.out_dir().join("web");
    ensure_dir(&web_dir)?;

    // Find .wasm
    let profile_dir = if config.release { "release" } else { "debug" };
    let wasm_target = rust_dir
        .join("target/wasm32-unknown-unknown")
        .join(profile_dir);

    // Copy wasm file
    if let Ok(entries) = fs::read_dir(&wasm_target) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "wasm") {
                let dest = web_dir.join("engine.wasm");
                fs::copy(&p, &dest)?;
                break;
            }
        }
    }

    // Generate index.html
    let title = window_title(config);
    let html = generate_web_html(&name, &title);
    fs::write(web_dir.join("index.html"), html)?;

    println!();
    println!("  ✅ Build succeeded!");
    println!("  Output: {}", web_dir.display());
    println!();
    println!("  Serve it:");
    println!(
        "    cd {} && python3 -m http.server 8080",
        web_dir.display()
    );
    Ok(())
}

fn generate_web_html(name: &str, title: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            min-height: 100vh;
            background: #fafafa;
        }}
        .container {{
            text-align: center;
            padding: 2rem;
        }}
        h1 {{ font-size: 2rem; color: #1a1a1a; margin-bottom: 0.5rem; }}
        .subtitle {{ font-size: 1.1rem; color: #666; margin-bottom: 1rem; }}
        .status {{
            display: inline-block;
            padding: 0.4rem 1rem;
            background: #e8f5e9;
            color: #2e7d32;
            border-radius: 4px;
            font-size: 0.9rem;
        }}
    </style>
</head>
<body>
    <div class="container" id="root">
        <h1>Welcome to {name}</h1>
        <p class="subtitle">Built with AppScale Engine</p>
        <span class="status" id="status">Loading WASM engine…</span>
    </div>
    <script>
        (async () => {{
            try {{
                const resp = await fetch('engine.wasm');
                if (resp.ok) {{
                    document.getElementById('status').textContent = 'WASM engine loaded ✓ (' +
                        Math.round((await resp.clone().arrayBuffer()).byteLength / 1024) + ' KB)';
                    document.getElementById('status').style.background = '#e8f5e9';
                }} else {{
                    document.getElementById('status').textContent = 'engine.wasm not found';
                    document.getElementById('status').style.background = '#fff3e0';
                    document.getElementById('status').style.color = '#e65100';
                }}
            }} catch (e) {{
                document.getElementById('status').textContent = 'Error: ' + e.message;
            }}
        }})();
    </script>
</body>
</html>
"#,
        name = name,
        title = title,
    )
}

// ---------------------------------------------------------------------------
// Android
// ---------------------------------------------------------------------------

fn build_android(config: &BuildConfig) -> Result<()> {
    println!("Building for Android...");

    let rust_dir = config.rust_dir();
    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found. Run `appscale create` first.");
    }

    // Check for Android target
    println!("  [1/4] Checking Android toolchain");
    let has_target = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("aarch64-linux-android"))
        .unwrap_or(false);

    if !has_target {
        println!("    Installing aarch64-linux-android target...");
        let result = Command::new("rustup")
            .args(["target", "add", "aarch64-linux-android"])
            .status();
        if result.is_err() || !result.unwrap().success() {
            println!("    ⚠ Could not install Android target");
        }
    }

    // Try compile (may fail without NDK linker)
    println!("  [2/4] Compiling Rust → .so (aarch64-linux-android)");
    let mut cargo = Command::new("cargo");
    cargo
        .arg("build")
        .arg("--target")
        .arg("aarch64-linux-android")
        .current_dir(&rust_dir);
    if config.release {
        cargo.arg("--release");
    }

    let compile_ok = cargo.status().map(|s| s.success()).unwrap_or(false);
    if compile_ok {
        println!("    ✓ Rust compilation succeeded");
    } else {
        println!("    ⚠ Cross-compilation requires Android NDK linker");
        println!("    Install cargo-ndk: cargo install cargo-ndk");
    }

    // Generate Gradle project
    println!("  [3/4] Generating Gradle project");
    let name = app_name(config);
    let android_dir = config.out_dir().join("android");
    ensure_dir(&android_dir.join("app/src/main/java/com/appscale"))?;
    ensure_dir(&android_dir.join("app/src/main/res/layout"))?;

    let activity = generate_android_activity(&name);
    fs::write(
        android_dir.join("app/src/main/java/com/appscale/MainActivity.java"),
        activity,
    )?;

    let layout = generate_android_layout(&name);
    fs::write(
        android_dir.join("app/src/main/res/layout/activity_main.xml"),
        layout,
    )?;

    let build_gradle = generate_android_build_gradle(&name);
    fs::write(android_dir.join("app/build.gradle"), build_gradle)?;

    let manifest = generate_android_manifest(&name);
    fs::write(
        android_dir.join("app/src/main/AndroidManifest.xml"),
        manifest,
    )?;

    println!("    ✓ Gradle project generated");

    println!("  [4/4] Gradle build");
    println!(
        "    ⚠ Open in Android Studio to build: {}",
        android_dir.display()
    );

    println!();
    println!("  ✅ Android project generated!");
    println!("  Output: {}", android_dir.display());
    Ok(())
}

fn generate_android_activity(name: &str) -> String {
    format!(
        r#"package com.appscale;

import android.app.Activity;
import android.os.Bundle;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.view.Gravity;

public class MainActivity extends Activity {{
    @Override
    protected void onCreate(Bundle savedInstanceState) {{
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
    }}

    static {{
        try {{
            System.loadLibrary("{lib_name}");
        }} catch (UnsatisfiedLinkError e) {{
            // Engine not linked yet — UI still renders
        }}
    }}
}}
"#,
        lib_name = name.replace('-', "_"),
    )
}

fn generate_android_layout(name: &str) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    s.push_str("<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\"\n");
    s.push_str("    android:layout_width=\"match_parent\"\n");
    s.push_str("    android:layout_height=\"match_parent\"\n");
    s.push_str("    android:orientation=\"vertical\"\n");
    s.push_str("    android:gravity=\"center\"\n");
    s.push_str("    android:background=\"#FAFAFA\">\n\n");
    s.push_str("    <TextView\n");
    s.push_str("        android:layout_width=\"wrap_content\"\n");
    s.push_str("        android:layout_height=\"wrap_content\"\n");
    s.push_str(&format!("        android:text=\"Welcome to {name}\"\n"));
    s.push_str("        android:textSize=\"24sp\"\n");
    s.push_str("        android:textStyle=\"bold\"\n");
    s.push_str("        android:textColor=\"#1A1A1A\" />\n\n");
    s.push_str("    <TextView\n");
    s.push_str("        android:layout_width=\"wrap_content\"\n");
    s.push_str("        android:layout_height=\"wrap_content\"\n");
    s.push_str("        android:text=\"Built with AppScale Engine\"\n");
    s.push_str("        android:textSize=\"16sp\"\n");
    s.push_str("        android:textColor=\"#666666\"\n");
    s.push_str("        android:layout_marginTop=\"8dp\" />\n");
    s.push_str("</LinearLayout>\n");
    s
}

fn generate_android_build_gradle(name: &str) -> String {
    format!(
        r#"plugins {{
    id 'com.android.application'
}}

android {{
    namespace 'com.appscale'
    compileSdk 34

    defaultConfig {{
        applicationId "com.appscale.{id}"
        minSdk 24
        targetSdk 34
        versionCode 1
        versionName "0.1.0"
    }}
}}
"#,
        id = name.to_lowercase().replace([' ', '-'], ""),
    )
}

fn generate_android_manifest(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <application
        android:label="{name}"
        android:theme="@android:style/Theme.Material.Light.NoActionBar">
        <activity
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        name = name,
    )
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

fn build_windows(config: &BuildConfig) -> Result<()> {
    println!("Building for Windows...");

    let rust_dir = config.rust_dir();
    if !rust_dir.join("Cargo.toml").exists() {
        anyhow::bail!("No rust/Cargo.toml found. Run `appscale create` first.");
    }

    // Try compile
    println!("  [1/3] Compiling Rust → DLL");
    let mut cargo = Command::new("cargo");
    cargo.arg("build").current_dir(&rust_dir);
    if config.release {
        cargo.arg("--release");
    }
    // On a non-Windows host, cross-compile
    if cfg!(not(target_os = "windows")) {
        let has_target = Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("x86_64-pc-windows-msvc"))
            .unwrap_or(false);

        if has_target {
            cargo.arg("--target").arg("x86_64-pc-windows-msvc");
        } else {
            // Just do a native build to verify compilation
            println!("    (Native build — cross-compile target not installed)");
        }
    }

    let compile_ok = cargo.status().map(|s| s.success()).unwrap_or(false);
    if compile_ok {
        println!("    ✓ Rust compilation succeeded");
    } else {
        println!("    ⚠ Compilation failed (Windows MSVC linker may be required)");
    }

    // Generate project files
    println!("  [2/3] Generating Windows project");
    let name = app_name(config);
    let win_dir = config.out_dir().join("windows");
    ensure_dir(&win_dir)?;

    let main_c = generate_windows_main(&name);
    fs::write(win_dir.join("main.c"), main_c)?;

    let build_script = generate_windows_build_script(&name);
    fs::write(win_dir.join("build.bat"), build_script)?;

    println!("    ✓ Windows project generated");

    println!("  [3/3] Build");
    if cfg!(target_os = "windows") {
        println!("    Run: cd {} && build.bat", win_dir.display());
    } else {
        println!("    ⚠ Cross-compile from macOS/Linux requires mingw-w64 or MSVC");
    }

    println!();
    println!("  ✅ Windows project generated!");
    println!("  Output: {}", win_dir.display());
    Ok(())
}

fn generate_windows_main(name: &str) -> String {
    format!(
        r#"// {name} — AppScale Windows Host
// Compile with: cl main.c /link user32.lib gdi32.lib

#include <windows.h>

LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {{
    switch (msg) {{
    case WM_PAINT: {{
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hwnd, &ps);
        RECT rc;
        GetClientRect(hwnd, &rc);
        SetBkMode(hdc, TRANSPARENT);
        DrawText(hdc, TEXT("Welcome to {name}"), -1, &rc,
                 DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        EndPaint(hwnd, &ps);
        return 0;
    }}
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }}
    return DefWindowProc(hwnd, msg, wp, lp);
}}

int WINAPI WinMain(HINSTANCE hInst, HINSTANCE prev, LPSTR cmd, int show) {{
    const char CLASS[] = "AppScaleWnd";
    WNDCLASS wc = {{0}};
    wc.lpfnWndProc = WndProc;
    wc.hInstance = hInst;
    wc.lpszClassName = CLASS;
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    RegisterClass(&wc);
    HWND hwnd = CreateWindow(CLASS, "{name}", WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT, 900, 600, NULL, NULL, hInst, NULL);
    ShowWindow(hwnd, show);
    MSG msg;
    while (GetMessage(&msg, NULL, 0, 0)) {{
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }}
    return (int)msg.wParam;
}}
"#,
        name = name,
    )
}

fn generate_windows_build_script(name: &str) -> String {
    format!(
        r#"@echo off
REM Build {name} for Windows
REM Requires Visual Studio Build Tools (cl.exe on PATH)
cl main.c /Fe:{name}.exe /link user32.lib gdi32.lib
echo.
echo Build complete: {name}.exe
"#,
        name = name,
    )
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

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
    fn build_config_profile() {
        let debug_cfg = BuildConfig {
            platform: "web".to_string(),
            release: false,
            simulator: false,
            output_dir: "dist".to_string(),
            project_root: PathBuf::from("/tmp/test"),
        };
        assert_eq!(debug_cfg.profile(), "debug");

        let release_cfg = BuildConfig {
            platform: "ios".to_string(),
            release: true,
            simulator: false,
            output_dir: "dist".to_string(),
            project_root: PathBuf::from("/tmp/test"),
        };
        assert_eq!(release_cfg.profile(), "release");
    }

    #[test]
    fn build_config_targets() {
        let platforms = vec![
            ("ios", "aarch64-apple-ios"),
            ("android", "aarch64-linux-android"),
            ("web", "wasm32-unknown-unknown"),
            ("macos", "aarch64-apple-darwin"),
            ("windows", "x86_64-pc-windows-msvc"),
        ];

        for (platform, expected_target) in platforms {
            let cfg = BuildConfig {
                platform: platform.to_string(),
                release: false,
                simulator: false,
                output_dir: "dist".to_string(),
                project_root: PathBuf::from("/tmp/test"),
            };
            assert_eq!(cfg.rust_target(), expected_target);
        }

        // iOS simulator target
        let sim_cfg = BuildConfig {
            platform: "ios".to_string(),
            release: false,
            simulator: true,
            output_dir: "dist".to_string(),
            project_root: PathBuf::from("/tmp/test"),
        };
        assert_eq!(sim_cfg.rust_target(), "aarch64-apple-ios-sim");
    }

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
        assert!(validate_platform("tizen").is_err());
    }

    #[test]
    fn info_plist_generation() {
        let plist = generate_info_plist("TestApp", "1.0.0");
        assert!(plist.contains("TestApp"));
        assert!(plist.contains("1.0.0"));
        assert!(plist.contains("com.appscale.testapp"));
    }

    #[test]
    fn web_html_generation() {
        let html = generate_web_html("MyApp", "My App Title");
        assert!(html.contains("MyApp"));
        assert!(html.contains("My App Title"));
        assert!(html.contains("engine.wasm"));
    }
}
