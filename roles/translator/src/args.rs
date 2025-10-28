//! Defines the structure and parsing logic for command-line arguments.
//!
//! It provides the `Args` struct to hold parsed arguments,
//! and the `from_args` function to parse them from the command line.
use clap::Parser;
use ext_config::{Config, File, FileFormat};
use shared_config::MinerGlobalConfig;
use std::path::PathBuf;
use tracing::error;
use translator_sv2::{config::TranslatorConfig, error::TproxyError};

const DEFAULT_MINIMUM_DIFFICULTY: u32 = 32;

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

    let mut minimum_difficulty = DEFAULT_MINIMUM_DIFFICULTY;
    let mut source: Option<String> = None;

    if let Some(global_config_path) = args.global_config_path.as_ref() {
        let global_config_str = global_config_path.to_str().ok_or_else(|| {
            error!("Invalid global configuration path.");
            TproxyError::BadCliArgs
        })?;

        match MinerGlobalConfig::from_path(global_config_str) {
            Ok(global) => {
                // Fill in mint configuration from shared config when missing locally
                if config.mint.is_none() {
                    config.mint = Some(global.mint.clone());
                    eprintln!(
                        "✅ Loaded mint config from shared config: {}",
                        global.mint.url
                    );
                }

                if let Some(ehash) = global.ehash {
                    minimum_difficulty = ehash.minimum_difficulty;
                } else {
                    eprintln!(
                        "Warning: no [ehash] section found in shared config {}; falling back to default difficulty {}",
                        global_config_str, DEFAULT_MINIMUM_DIFFICULTY
                    );
                }

                source = Some(global_config_str.to_string());
            }
            Err(err) => {
                eprintln!(
                    "Warning: failed to parse shared global config {} ({}); using default difficulty {}",
                    global_config_str, err, DEFAULT_MINIMUM_DIFFICULTY
                );
                source = Some(global_config_str.to_string());
            }
        }
    }

    config
        .downstream_difficulty_config
        .set_min_hashrate_from_difficulty(minimum_difficulty);

    if let Some(source_path) = source {
        eprintln!(
            "Derived min_individual_miner_hashrate from {} using minimum_difficulty = {}",
            source_path, minimum_difficulty
        );
    } else {
        eprintln!(
            "Using default minimum_difficulty = {} to derive min_individual_miner_hashrate",
            minimum_difficulty
        );
    }

    config.set_log_dir(args.log_file);

    Ok(config)
}
