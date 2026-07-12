use clap::{Parser, ValueEnum};
use std::path::PathBuf;

pub const DEFAULT_ELECTRON_VERSION: &str = "41.2.2";

#[derive(Parser, Debug)]
#[command(
    name = "chatgpt-linux-desktop",
    version,
    about = "Repack the official ChatGPT Classic Windows MSIX into a native Linux package",
    after_help = "EXAMPLES:\n  chatgpt-linux-desktop --format deb\n  chatgpt-linux-desktop --msix ./ChatGPT.msixbundle --no-clean\n  chatgpt-linux-desktop --pkg-version 1.2026.100"
)]
pub struct Args {
    /// Path to the ChatGPT Classic MSIXBundle.
    /// If omitted, the tool auto-fetches via rg-adguard.
    #[arg(long, value_name = "PATH")]
    pub msix: Option<PathBuf>,

    /// Store URL/Product ID used for rg-adguard auto-fetch.
    /// Default is the official ChatGPT Classic Store listing.
    #[arg(
        long,
        value_name = "QUERY",
        default_value = "https://apps.microsoft.com/detail/9NT1R1C2HH7J"
    )]
    pub store_query: String,

    /// Update ring used by rg-adguard.
    ///
    /// retail is the stable Microsoft Store ring. rp, wif, and wis are
    /// Microsoft insider rings and may return preview packages.
    #[arg(
        long,
        value_enum,
        default_value = "retail",
        help = "Update ring: retail (stable), rp (Release Preview), wif (Windows Insider Fast), wis (Windows Insider Slow)"
    )]
    pub ring: Ring,

    /// Override the detected package version
    #[arg(long = "pkg-version", value_name = "VERSION")]
    pub pkg_version: Option<String>,

    /// Output directory for built packages
    #[arg(long, value_name = "DIR", default_value = "dist")]
    pub out_dir: PathBuf,

    /// Package format to build
    #[arg(long, value_enum, default_value = "arch")]
    pub format: Format,

    /// Electron version to bundle (from GitHub releases)
    #[arg(long, value_name = "VERSION", default_value = DEFAULT_ELECTRON_VERSION)]
    pub electron_version: String,

    /// Keep the build directory after completion
    #[arg(long)]
    pub no_clean: bool,

    /// Package maintainer string (MAINTAINER env var overrides the default)
    #[arg(long, default_value = "Local Build", env = "MAINTAINER")]
    pub maintainer: String,
}

#[derive(Clone, Debug, ValueEnum, PartialEq)]
pub enum Format {
    Arch,
    Deb,
    Rpm,
    Both,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum Ring {
    Retail,
    Rp,
    Wif,
    Wis,
}

impl Ring {
    pub fn as_str(&self) -> &'static str {
        // rg-adguard expects uppercase ring codes, not the clap ValueEnum names.
        match self {
            Ring::Retail => "Retail",
            Ring::Rp => "RP",
            Ring::Wif => "WIF",
            Ring::Wis => "WIS",
        }
    }
}

impl Format {
    pub fn builds_arch(&self) -> bool {
        matches!(self, Format::Arch | Format::Both)
    }
    pub fn builds_deb(&self) -> bool {
        matches!(self, Format::Deb | Format::Both)
    }
    pub fn builds_rpm(&self) -> bool {
        matches!(self, Format::Rpm | Format::Both)
    }
}
