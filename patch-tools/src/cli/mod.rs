pub mod app;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "patch-tools", about = "Android package analysis CLI")]
pub struct Cli {
    /// Output format
    #[arg(long, default_value = "markdown", global = true)]
    pub format: OutputFormat,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Human,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage the daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Load an APK, APKM, or XAPK into the running daemon
    Load {
        /// Path to APK, APKM, or XAPK file
        apk_path: PathBuf,
    },
    /// Unload an APK
    Unload {
        /// APK selector (package name, package/version, or internal ID). Optional when one APK is loaded.
        apk: Option<String>,
    },
    /// Execute a Kotlin script against loaded APKs
    Run {
        /// Path to .kts script
        script_path: PathBuf,
        /// Save and install patched APKs via adb after a successful run
        #[arg(long)]
        install: bool,
        /// adb device serial to target
        #[arg(long)]
        device: Option<String>,
    },
    /// Create main.kts and AGENTS.md in the current directory
    Scaffold,
    /// Generate fingerprints for a method
    #[command(override_usage = "patch-tools fingerprint [OPTIONS] [APK] <METHOD_ID>")]
    Fingerprint {
        /// Method selector, or APK selector followed by method selector
        #[arg(required = true, value_name = "APK_OR_METHOD_ID", num_args = 1..=2)]
        args: Vec<String>,
        /// Maximum number of fingerprints to return after ranking
        #[arg(long, short = 'n', default_value_t = 8)]
        limit: u32,
    },
    /// Generate class fingerprints that can be used as `classFingerprint = ...`
    #[command(override_usage = "patch-tools class-fingerprint [OPTIONS] [APK] <CLASS_ID>")]
    ClassFingerprint {
        /// Class selector, or APK selector followed by class selector
        #[arg(required = true, value_name = "APK_OR_CLASS_ID", num_args = 1..=2)]
        args: Vec<String>,
        /// Maximum number of fingerprints to return after ranking
        #[arg(long, short = 'n', default_value_t = 8)]
        limit: u32,
    },
    /// Search methods across loaded APKs
    Search {
        /// Search query terms. Multiple values are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Maximum number of results to return per APK
        #[arg(long, short = 'n', default_value_t = 8)]
        limit: u32,
    },
    /// Map a method from one loaded APK to similar methods in another loaded APK
    Map {
        /// Source APK selector (package name, package/version, or internal ID)
        old_apk: String,
        /// Source method selector
        method_id: String,
        /// Target APK selector (package name, package/version, or internal ID)
        new_apk: String,
        /// Maximum number of similar methods to return
        #[arg(long, short = 'n', default_value_t = 8)]
        limit: u32,
    },
    /// Get smali source for a method
    #[command(override_usage = "patch-tools smali [OPTIONS] [APK] <METHOD_ID>")]
    Smali {
        /// Method selector, or APK selector followed by method selector
        #[arg(required = true, value_name = "APK_OR_METHOD_ID", num_args = 1..=2)]
        args: Vec<String>,
    },
    /// Generate shell completion scripts
    #[command(visible_alias = "completions")]
    Completion {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Internal: run as daemon process (hidden)
    #[command(hide = true)]
    InternalDaemon,
}

#[derive(Subcommand)]
pub enum DaemonAction {
    /// Start the daemon
    Start {
        /// APK, APKM, or XAPK files to preload
        #[arg(long)]
        apk: Vec<PathBuf>,
    },
    /// Stop the daemon
    Stop,
    /// Query daemon status
    Status,
}
