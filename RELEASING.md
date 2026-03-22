# Releasing the AppScale CLI

This document covers how to publish the `appscale` CLI binary so users can install it via **Homebrew**, **cargo install**, **curl**, **npm**, or **Scoop (Windows)**.

---

## Installation Methods (for end users)

Once released, users install via any of these:

```bash
# 1. Homebrew (macOS / Linux)
brew tap subham11/tap
brew install appscale

# 2. Cargo (any OS with Rust installed)
cargo install appscale-cli

# 3. Shell script (macOS / Linux)
curl -fsSL https://raw.githubusercontent.com/subham11/appscale-engine/main/install.sh | sh

# 4. npm (any OS with Node.js)
npm install -g @appscale/cli

# 5. Scoop (Windows)
scoop bucket add appscale https://github.com/subham11/scoop-appscale
scoop install appscale

# 6. Direct download
# Download from GitHub Releases:
# https://github.com/subham11/appscale-engine/releases
```

---

## Release Process (for maintainers)

### Step 1: Bump the Version

Edit the single version source in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.2.0"  # ← Update this
```

Both `appscale-core` and `appscale-cli` inherit this version automatically.

### Step 2: Commit and Tag

```bash
git add -A
git commit -m "release: v0.2.0"
git tag v0.2.0
git push origin main --tags
```

### Step 3: Automated Release Pipeline

Pushing a `v*.*.*` tag triggers `.github/workflows/release.yml`, which:

| Stage | What Happens |
|-------|-------------|
| **Build** | Compiles release binaries for 5 targets (macOS arm64, macOS x86_64, Linux x86_64, Linux arm64, Windows x86_64) |
| **Package** | Creates `.tar.gz` (Unix) or `.zip` (Windows) archives with SHA256 checksums |
| **GitHub Release** | Creates a release with auto-generated notes + all binary assets |
| **crates.io** | Publishes `appscale-core` then `appscale-cli` |
| **Homebrew** | Dispatches an event to update the Homebrew tap formula |

### Build Targets

| Platform | Rust Target | Archive Format |
|----------|-------------|----------------|
| macOS (Apple Silicon) | `aarch64-apple-darwin` | `.tar.gz` |
| macOS (Intel) | `x86_64-apple-darwin` | `.tar.gz` |
| Linux (x86_64) | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Linux (ARM64) | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| Windows (x86_64) | `x86_64-pc-windows-msvc` | `.zip` |

---

## Setting Up Each Distribution Channel

### 1. Homebrew Tap

Create a separate repository `subham11/homebrew-tap` with this structure:

```
homebrew-tap/
└── Formula/
    └── appscale.rb
```

The formula template is already in `Formula/appscale.rb` in this repo. Copy it to the tap repo and replace the `PLACEHOLDER_*_SHA256` values with real checksums from the release.

**Automated updates:** The release workflow dispatches a `repository-dispatch` event to the tap repo. Set up a workflow in `homebrew-tap` to receive it:

```yaml
# In subham11/homebrew-tap/.github/workflows/update.yml
name: Update Formula
on:
  repository_dispatch:
    types: [update-formula]
jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Update formula
        run: |
          VERSION="${{ github.event.client_payload.version }}"
          ARM64_SHA="${{ github.event.client_payload.arm64_sha }}"
          X86_64_SHA="${{ github.event.client_payload.x86_64_sha }}"
          
          sed -i "s/version \".*\"/version \"${VERSION}\"/" Formula/appscale.rb
          sed -i "s/PLACEHOLDER_ARM64_SHA256/${ARM64_SHA}/" Formula/appscale.rb
          sed -i "s/PLACEHOLDER_X86_64_SHA256/${X86_64_SHA}/" Formula/appscale.rb
      - name: Commit and push
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add Formula/appscale.rb
          git commit -m "Update appscale to ${{ github.event.client_payload.version }}"
          git push
```

**Required secret:** Add a `HOMEBREW_TAP_TOKEN` secret to the main repo — a GitHub PAT with write access to the tap repo.

### 2. crates.io

One-time setup:

```bash
# Login to crates.io
cargo login
# This saves your API token to ~/.cargo/credentials.toml
```

Add the token as `CARGO_REGISTRY_TOKEN` in **GitHub repo → Settings → Secrets → Actions**.

Before first publish, verify the crate can be packaged:

```bash
cargo publish --package appscale-core --dry-run
cargo publish --package appscale-cli --dry-run
```

**Note:** The `appscale-core` dependency in `cli/Cargo.toml` uses `path = "../rust-core"`. For crates.io, you must also add a version:

```toml
[dependencies]
appscale-core = { path = "../rust-core", version = "0.1.0" }
```

### 3. npm Wrapper (optional)

Create a lightweight npm package that downloads the correct binary at install time. Create a new repo or folder `npm/`:

```
npm/
├── package.json
├── install.js    # postinstall script — downloads the right binary
└── bin/
    └── appscale  # shell wrapper
```

**`package.json`:**

```json
{
  "name": "@appscale/cli",
  "version": "0.1.0",
  "description": "AppScale CLI — Cross-platform React UI engine",
  "bin": { "appscale": "bin/appscale" },
  "scripts": { "postinstall": "node install.js" },
  "os": ["darwin", "linux", "win32"],
  "cpu": ["x64", "arm64"]
}
```

**`install.js`** downloads the binary from GitHub Releases based on `process.platform` and `process.arch`, then places it in `bin/`.

### 4. Scoop (Windows)

Create a repo `subham11/scoop-appscale` with a manifest:

```json
{
  "version": "0.1.0",
  "description": "AppScale CLI — Cross-platform React UI engine",
  "homepage": "https://github.com/subham11/appscale-engine",
  "license": "MIT",
  "architecture": {
    "64bit": {
      "url": "https://github.com/subham11/appscale-engine/releases/download/v0.1.0/appscale-v0.1.0-x86_64-pc-windows-msvc.zip",
      "hash": "PLACEHOLDER_SHA256"
    }
  },
  "bin": "appscale.exe"
}
```

Users then run:

```powershell
scoop bucket add appscale https://github.com/subham11/scoop-appscale
scoop install appscale
```

---

## Quick Checklist for First Release

- [x] Create the `subham11/homebrew-tap` repo with `Formula/appscale.rb`
- [x] Add `HOMEBREW_TAP_TOKEN` secret to the main repo (GitHub PAT → tap repo write access)
- [x] Add `CARGO_REGISTRY_TOKEN` secret to the main repo (from `cargo login`)
- [x] Update `cli/Cargo.toml` to include version alongside path: `appscale-core = { path = "../rust-core", version = "0.1.0" }`
- [x] Run `cargo publish --dry-run` for both crates to verify packaging
- [x] Create a `LICENSE` file (MIT) in the repo root
- [x] Tag and push: `git tag v0.1.0 && git push origin main --tags`
- [ ] Verify GitHub Release appears with all 5 binary archives
- [ ] Verify `brew install subham11/tap/appscale` works
- [ ] Verify `cargo install appscale-cli` works

---

## Local Testing

Build a release binary locally to test before pushing a tag:

```bash
# Build release binary for current platform
cargo build --release --package appscale-cli

# Binary is at:
./target/release/appscale

# Test it
./target/release/appscale --version
./target/release/appscale create test-project
```

---

## Version Bump Workflow

```
1. Edit Cargo.toml   →   [workspace.package] version = "X.Y.Z"
2. git commit         →   "release: vX.Y.Z"
3. git tag vX.Y.Z     →   triggers release.yml
4. CI builds          →   5 binaries packaged + checksummed
5. GitHub Release     →   auto-created with assets
6. crates.io          →   both crates published
7. Homebrew           →   tap formula auto-updated
```

Every release is just: **edit version → commit → tag → push**.
