use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "chatgpt-linux-desktop",
    about = "Repack the official ChatGPT Windows MSIX into a native Linux package"
)]
pub struct Args {
    /// Path to the ChatGPT MSIXBundle.
    /// If omitted, the tool auto-fetches via rg-adguard.
    #[arg(long, value_name = "PATH")]
    pub msix: Option<PathBuf>,

    /// Store URL/Product ID used for rg-adguard auto-fetch.
    #[arg(
        long,
        value_name = "QUERY",
        default_value = "https://apps.microsoft.com/detail/9NT1R1C2HH7J"
    )]
    pub store_query: String,

    /// Update ring used by rg-adguard
    #[arg(long, value_enum, default_value = "retail")]
    pub ring: Ring,

    /// Override the detected package version
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,

    /// Output directory for built packages
    #[arg(long, value_name = "DIR", default_value = "dist")]
    pub out_dir: PathBuf,

    /// Package format to build
    #[arg(long, value_enum, default_value = "rpm")]
    pub format: Format,

    /// Electron version to bundle (from GitHub releases)
    #[arg(long, value_name = "VERSION", default_value = "41.2.2")]
    pub electron_version: String,

    /// Keep the build directory after completion
    #[arg(long)]
    pub no_clean: bool,

    /// Package maintainer string
    #[arg(long, default_value = "Local Build", env = "MAINTAINER")]
    pub maintainer: String,
}

#[derive(Clone, Debug, ValueEnum, PartialEq)]
pub enum Format {
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
        match self {
            Ring::Retail => "Retail",
            Ring::Rp => "RP",
            Ring::Wif => "WIF",
            Ring::Wis => "WIS",
        }
    }
}

impl Format {
    pub fn builds_deb(&self) -> bool {
        matches!(self, Format::Deb | Format::Both)
    }
    pub fn builds_rpm(&self) -> bool {
        matches!(self, Format::Rpm | Format::Both)
    }
}
