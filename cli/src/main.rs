//! AppScale CLI — Project scaffolding, development server, and build system.
//!
//! Commands:
//! - `appscale create <project>` — Scaffold a new AppScale project
//! - `appscale dev` — Launch development server with hot reload
//! - `appscale build <platform>` — Production build for a target platform

use anyhow::Result;
use clap::{Parser, Subcommand};

mod build;
mod create;
mod dev;

/// AppScale — Cross-platform React UI engine
#[derive(Parser)]
#[command(name = "appscale", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new AppScale project
    Create {
        /// Project name (will be used as directory name)
        name: String,

        /// Template to use
        #[arg(long, default_value = "default")]
        template: String,
    },

    /// Start the development server with hot reload
    Dev {
        /// Port for the dev server
        #[arg(short, long, default_value_t = 8081)]
        port: u16,

        /// Platform to target during development
        #[arg(short = 't', long, default_value = "web")]
        platform: String,
    },

    /// Build the project for a target platform
    Build {
        /// Target platform: ios, android, web, macos, windows
        platform: String,

        /// Build in release mode
        #[arg(long)]
        release: bool,

        /// Output directory
        #[arg(short, long, default_value = "dist")]
        output: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Create { name, template } => create::run(&name, &template),
        Commands::Dev { port, platform } => dev::run(port, &platform),
        Commands::Build {
            platform,
            release,
            output,
        } => build::run(&platform, release, &output),
    }
}
