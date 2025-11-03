//! Defines the structure and parsing logic for command-line arguments.
//!
//! It provides the `Args` struct to hold parsed arguments,
//! and the `from_args` function to parse them from the command line.
use clap::Parser;
use ext_config::{Config, File, FileFormat};
use std::path::PathBuf;
use tracing::error;
use translator_sv2::{config::TranslatorConfig, error::TproxyError};

/// Holds the parsed CLI arguments.
#[derive(Parser, Debug)]
#[command(author, version, about = "Translator Proxy", long_about = None)]
pub struct Args {
    #[arg(
        short = 'c',
        long = "config",
        help = "Path to the TOML configuration file",
        default_value = "proxy-config.toml"
    )]
    pub config_path: PathBuf,
    #[arg(
        short = 'g',
        long = "global-config",
        help = "Path to the global shared configuration file (optional)"
    )]
    pub global_config_path: Option<PathBuf>,
    #[arg(
        short = 'f',
        long = "log-file",
        help = "Path to the log file. If not set, logs will only be written to stdout."
    )]
    pub log_file: Option<PathBuf>,
}

/// Process CLI args, if any.
#[allow(clippy::result_large_err)]
pub fn process_cli_args() -> Result<TranslatorConfig, TproxyError> {
    // Parse CLI arguments
    let args = Args::parse();

    // Build configuration from the provided file path
    let config_path = args.config_path.to_str().ok_or_else(|| {
        error!("Invalid configuration path.");
        TproxyError::BadCliArgs
    })?;

    let settings = Config::builder()
        .add_source(File::new(config_path, FileFormat::Toml))
        .build()?;

    // Deserialize settings into TranslatorConfig
    let mut config = settings.try_deserialize::<TranslatorConfig>()?;

    config.set_log_dir(args.log_file);

    // Load stats poll interval from global config if provided
    if let Some(global_config_path) = args.global_config_path {
        let global_config_str = global_config_path
            .to_str()
            .ok_or_else(|| TproxyError::BadCliArgs)?;
        if let Ok(settings) = Config::builder()
            .add_source(File::new(global_config_str, FileFormat::Toml))
            .build()
        {
            // Try to extract snapshot_poll_interval_secs from [stats] section
            if let Ok(interval) = settings.get_int("stats.snapshot_poll_interval_secs") {
                config.set_snapshot_poll_interval_secs(interval as u64);
            }
        }
    }

    Ok(config)
}
