//! CLI argument parsing for the Pool binary.
//!
//! Defines the `Args` struct and a function to process CLI arguments into a PoolConfig.

use clap::Parser;
use ext_config::{Config, File, FileFormat};
use pool_sv2::config::PoolConfig;
use shared_config::PoolGlobalConfig;
use std::path::PathBuf;

/// Holds the parsed CLI arguments for the Pool binary.
#[derive(Parser, Debug)]
#[command(author, version, about = "Pool CLI", long_about = None)]
pub struct Args {
    #[arg(
        short = 'c',
        long = "config",
        help = "Path to the TOML configuration file",
        default_value = "pool-config.toml"
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

/// Parses CLI arguments and loads the PoolConfig from the specified file.
pub fn process_cli_args() -> PoolConfig {
    let args = Args::parse();
    let config_path = args.config_path.to_str().expect("Invalid config path");
    let mut config: PoolConfig = Config::builder()
        .add_source(File::new(config_path, FileFormat::Toml))
        .build()
        .and_then(|settings| settings.try_deserialize::<PoolConfig>())
        .expect("Failed to load or deserialize config");

    // Load locking_pubkey from global config if provided
    if let Some(global_config_path) = args.global_config_path {
        let global_config_str = global_config_path
            .to_str()
            .expect("Invalid global config path");
        if let Ok(settings) = Config::builder()
            .add_source(File::new(global_config_str, FileFormat::Toml))
            .build()
        {
            // Try to extract locking_pubkey from [locking] section
            if let Ok(locking_pubkey) = settings.get_string("locking.locking_pubkey") {
                eprintln!(
                    "✅ Loaded locking_pubkey from global config: {}",
                    locking_pubkey
                );
                config.set_locking_pubkey(locking_pubkey);
            } else {
                eprintln!("⚠️  No locking_pubkey found in global config [locking] section");
            }
        } else {
            eprintln!(
                "⚠️  Failed to load global config file: {}",
                global_config_str
            );
        }

        match PoolGlobalConfig::from_path(global_config_str) {
            Ok(shared) => {
                config.set_sv2_messaging(shared.sv2_messaging.clone());
                config.set_minimum_difficulty(shared.ehash.map(|e| e.minimum_difficulty));
                config.set_mint_http_url(Some(shared.mint.url));
            }
            Err(err) => {
                eprintln!(
                    "⚠️  Failed to parse shared global config ({}): {}",
                    global_config_str, err
                );
            }
        }
    }

    config.set_log_dir(args.log_file);

    config
}
