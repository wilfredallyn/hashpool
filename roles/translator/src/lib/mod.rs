//! ## Translator Sv2
//!
//! Provides the core logic and main struct (`TranslatorSv2`) for running a
//! Stratum V1 to Stratum V2 translation proxy.
//!
//! This module orchestrates the interaction between downstream SV1 miners and upstream SV2
//! applications (proxies or pool servers).
//!
//! The central component is the `TranslatorSv2` struct, which encapsulates the state and
//! provides the `start` method as the main entry point for running the translator service.
//! It relies on several sub-modules (`config`, `downstream_sv1`, `upstream_sv2`, `proxy`, `status`,
//! etc.) for specialized functionalities.
#![allow(clippy::module_inception)]
use anyhow::{Context, Result};
use async_channel::unbounded;
use bip39::Mnemonic;
use cdk::{
    nuts::{CurrencyUnit, SecretKey},
    wallet::Wallet,
    Amount,
};
use cdk_sqlite::WalletSqliteDatabase;
use std::{net::SocketAddr, path::Path, str::FromStr, sync::Arc};
use tokio::{sync::mpsc, time::Duration};
use tracing::{debug, error, info, warn};

pub use v1::server_to_client;

use config::TranslatorConfig;

use crate::{
    status::{State, Status},
    sv1::sv1_server::sv1_server::Sv1Server,
    sv2::{channel_manager::ChannelMode, ChannelManager, Upstream},
    task_manager::TaskManager,
    utils::ShutdownMessage,
};

pub mod config;
pub mod error;
pub mod faucet_api;
pub mod miner_stats;
pub mod stats_integration;
pub mod status;
pub mod sv1;
pub mod sv2;
mod task_manager;
pub mod utils;

/// The main struct that manages the SV1/SV2 translator.
#[derive(Clone)]
pub struct TranslatorSv2 {
    config: TranslatorConfig,
    wallet: Option<Arc<Wallet>>,
    miner_tracker: Arc<miner_stats::MinerTracker>,
}

impl std::fmt::Debug for TranslatorSv2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranslatorSv2")
            .field("config", &self.config)
            .field("wallet", &self.wallet.is_some())
            .field("miner_tracker", &"MinerTracker")
            .finish()
    }
}

impl TranslatorSv2 {
    /// Creates a new `TranslatorSv2`.
    ///
    /// Initializes the translator with the given configuration and sets up
    /// the reconnect wait time.
    pub fn new(config: TranslatorConfig) -> Self {
        Self {
            config,
            wallet: None,
            miner_tracker: Arc::new(miner_stats::MinerTracker::new()),
        }
    }

    /// Helper function to resolve and prepare DB paths
    fn resolve_and_prepare_db_path(config_path: &str) -> std::path::PathBuf {
        let path = Path::new(config_path);
        let full_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .expect("Failed to get current working directory")
                .join(path)
        };

        if let Some(parent) = full_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .expect("Failed to create parent directory for DB path");
            }
        }

        full_path
    }

    /// Creates and initializes a wallet for the translator
    async fn create_wallet(
        mint_url: String,
        mnemonic: String,
        db_path: String,
    ) -> Result<Arc<Wallet>> {
        debug!("Parsing mnemonic...");
        let seed = Mnemonic::from_str(&mnemonic)
            .with_context(|| format!("Invalid mnemonic: '{}'", mnemonic))?
            .to_seed_normalized("");
        let seed: [u8; 64] = seed
            .try_into()
            .map_err(|_| anyhow::anyhow!("Seed must be exactly 64 bytes"))?;
        debug!("Seed derived.");

        let db_path = Self::resolve_and_prepare_db_path(&db_path);
        debug!("Resolved db_path: {}", db_path.display());

        debug!("Creating localstore...");
        let localstore = WalletSqliteDatabase::new(db_path)
            .await
            .context("WalletSqliteDatabase::new failed")?;

        debug!("Creating wallet...");
        // TODO: Move "HASH" currency unit to configuration (Phase 2)
        let wallet = Wallet::new(
            &mint_url,
            CurrencyUnit::Custom("HASH".to_string()),
            Arc::new(localstore),
            seed,
            None,
        )
        .context("Failed to create wallet")?;
        debug!("Wallet created.");

        let balance = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(wallet.total_balance())
        });
        debug!("Wallet balance: {:?}", balance);

        Ok(Arc::new(wallet))
    }

    /// Starts the translator.
    ///
    /// This method starts the main event loop, which handles connections,
    /// protocol translation, job management, and status reporting.
    pub async fn start(mut self) {
        info!("Starting Translator Proxy...");

        let min_hashrate = self
            .config
            .downstream_difficulty_config
            .min_individual_miner_hashrate as f64;
        let shares_per_minute = self.config.downstream_difficulty_config.shares_per_minute;
        info!(
            "Downstream difficulty derived from minimum_difficulty = {} bits: min_hashrate ~ {:.3} GH/s (shares_per_minute = {:.2})",
            self.config
                .downstream_difficulty_config
                .minimum_difficulty_bits(),
            min_hashrate / 1_000_000_000.0,
            shares_per_minute
        );

        // Initialize and validate wallet config if mint is configured
        if self.config.mint.is_some() {
            self.config
                .wallet
                .initialize()
                .expect("Failed to initialize wallet config");

            let mint_url = self
                .config
                .mint
                .as_ref()
                .map(|m| m.url.clone())
                .expect("Mint URL required for wallet");

            let db_path = std::env::var("CDK_WALLET_DB_PATH")
                .unwrap_or_else(|_| self.config.wallet.db_path.clone());

            match Self::create_wallet(
                mint_url,
                self.config.wallet.mnemonic.clone(),
                db_path,
            )
            .await
            {
                Ok(wallet) => {
                    info!("Wallet initialized successfully");
                    self.wallet = Some(wallet);
                }
                Err(e) => {
                    error!("Failed to create wallet: {}", e);
                    // Continue without wallet - quote functionality won't work but translator can
                    // still function
                }
            }
        }

        let (notify_shutdown, _) = tokio::sync::broadcast::channel::<ShutdownMessage>(1);
        let (shutdown_complete_tx, mut shutdown_complete_rx) = mpsc::channel::<()>(1);
        let task_manager = Arc::new(TaskManager::new());

        if let Some(wallet) = self.wallet.clone() {
            self.spawn_quote_sweeper(&task_manager, wallet.clone());

            // Start faucet API for ehash minting
            let faucet_port = self.config.faucet_port;
            let faucet_timeout = self.config.faucet_timeout;
            task_manager.spawn(faucet_api::run_faucet_api(faucet_port, wallet, faucet_timeout));
        } else {
            debug!("Quote sweeper and faucet disabled: wallet not configured");
        }

        // Start snapshot-based stats polling loop to send stats to stats service
        let stats_addr_opt = self.config.stats_server_address.clone();
        let stats_poll_interval = self.config.snapshot_poll_interval_secs;
        let translator_clone = self.clone();
        if let Some(stats_addr) = stats_addr_opt {
            use stats::stats_adapter::StatsSnapshotProvider;
            use stats::stats_client::StatsClient;

            info!("Starting stats polling loop, sending to {} every {} seconds",
                  stats_addr, stats_poll_interval);

            let translator_for_stats = translator_clone.clone();
            let stats_addr_clone = stats_addr.clone();
            task_manager.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(stats_poll_interval));
                let status_client = StatsClient::new(stats_addr.clone());
                let metrics_client = StatsClient::new(stats_addr_clone);

                loop {
                    interval.tick().await;
                    let _ = status_client.send_snapshot(translator_for_stats.get_snapshot()).await;
                    let _ = metrics_client.send_snapshot(translator_for_stats.get_metrics_snapshot()).await;
                }
            });
        }

        let (status_sender, status_receiver) = async_channel::unbounded::<Status>();

        let (channel_manager_to_upstream_sender, channel_manager_to_upstream_receiver) =
            unbounded();
        let (upstream_to_channel_manager_sender, upstream_to_channel_manager_receiver) =
            unbounded();
        let (channel_manager_to_sv1_server_sender, channel_manager_to_sv1_server_receiver) =
            unbounded();
        let (sv1_server_to_channel_manager_sender, sv1_server_to_channel_manager_receiver) =
            unbounded();

        debug!("Channels initialized.");

        let upstream_addresses = self
            .config
            .upstreams
            .iter()
            .map(|upstream| {
                let upstream_addr =
                    SocketAddr::new(upstream.address.parse().unwrap(), upstream.port);
                (upstream_addr, upstream.authority_pubkey)
            })
            .collect::<Vec<_>>();

        let upstream = match Upstream::new(
            &upstream_addresses,
            upstream_to_channel_manager_sender.clone(),
            channel_manager_to_upstream_receiver.clone(),
            notify_shutdown.clone(),
            shutdown_complete_tx.clone(),
        )
        .await
        {
            Ok(upstream) => {
                debug!("Upstream initialized successfully.");
                upstream
            }
            Err(e) => {
                error!("Failed to initialize upstream connection: {e:?}");
                return;
            }
        };

        let channel_manager = Arc::new(ChannelManager::new(
            channel_manager_to_upstream_sender,
            upstream_to_channel_manager_receiver,
            channel_manager_to_sv1_server_sender.clone(),
            sv1_server_to_channel_manager_receiver,
            if self.config.aggregate_channels {
                ChannelMode::Aggregated
            } else {
                ChannelMode::NonAggregated
            },
            self.wallet.clone(),
        ));

        let downstream_addr = SocketAddr::new(
            self.config.downstream_address.parse().unwrap(),
            self.config.downstream_port,
        );

        let sv1_server = Arc::new(Sv1Server::new(
            downstream_addr,
            channel_manager_to_sv1_server_receiver,
            sv1_server_to_channel_manager_sender,
            self.config.clone(),
            self.miner_tracker.clone(),
        ));

        ChannelManager::run_channel_manager_tasks(
            channel_manager.clone(),
            notify_shutdown.clone(),
            shutdown_complete_tx.clone(),
            status_sender.clone(),
            task_manager.clone(),
        )
        .await;

        if let Err(e) = upstream
            .start(
                notify_shutdown.clone(),
                shutdown_complete_tx.clone(),
                status_sender.clone(),
                task_manager.clone(),
            )
            .await
        {
            error!("Failed to start upstream listener: {e:?}");
            return;
        }

        let notify_shutdown_clone = notify_shutdown.clone();
        let shutdown_complete_tx_clone = shutdown_complete_tx.clone();
        let status_sender_clone = status_sender.clone();
        let task_manager_clone = task_manager.clone();
        task_manager.spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        info!("Ctrl+C received — initiating graceful shutdown...");
                        let _ = notify_shutdown_clone.send(ShutdownMessage::ShutdownAll);
                        break;
                    }
                    message = status_receiver.recv() => {
                        if let Ok(status) = message {
                            match status.state {
                                State::DownstreamShutdown{downstream_id,..} => {
                                    warn!("Downstream {downstream_id:?} disconnected — notifying SV1 server.");
                                    let _ = notify_shutdown_clone.send(ShutdownMessage::DownstreamShutdown(downstream_id));
                                }
                                State::Sv1ServerShutdown(_) => {
                                    warn!("SV1 Server shutdown requested — initiating full shutdown.");
                                    let _ = notify_shutdown_clone.send(ShutdownMessage::ShutdownAll);
                                    break;
                                }
                                State::ChannelManagerShutdown(_) => {
                                    warn!("Channel Manager shutdown requested — initiating full shutdown.");
                                    let _ = notify_shutdown_clone.send(ShutdownMessage::ShutdownAll);
                                    break;
                                }
                                State::UpstreamShutdown(msg) => {
                                    warn!("Upstream connection dropped: {msg:?} — attempting reconnection...");

                                    match Upstream::new(
                                        &upstream_addresses,
                                        upstream_to_channel_manager_sender.clone(),
                                        channel_manager_to_upstream_receiver.clone(),
                                        notify_shutdown_clone.clone(),
                                        shutdown_complete_tx_clone.clone(),
                                    ).await {
                                        Ok(upstream) => {
                                            if let Err(e) = upstream
                                                .start(
                                                    notify_shutdown_clone.clone(),
                                                    shutdown_complete_tx_clone.clone(),
                                                    status_sender_clone.clone(),
                                                    task_manager_clone.clone()
                                                )
                                                .await
                                            {
                                                error!("Restarted upstream failed to start: {e:?}");
                                                let _ = notify_shutdown_clone.send(ShutdownMessage::ShutdownAll);
                                                break;
                                            } else {
                                                info!("Upstream restarted successfully.");
                                                // Reset channel manager state and shutdown downstreams in one message
                                                let _ = notify_shutdown_clone.send(ShutdownMessage::UpstreamReconnectedResetAndShutdownDownstreams);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to reinitialize upstream after disconnect: {e:?}");
                                            let _ = notify_shutdown_clone.send(ShutdownMessage::ShutdownAll);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        if let Err(e) = Sv1Server::start(
            sv1_server,
            notify_shutdown.clone(),
            shutdown_complete_tx.clone(),
            status_sender.clone(),
            task_manager.clone(),
        )
        .await
        {
            error!("SV1 server startup failed: {e:?}");
            notify_shutdown.send(ShutdownMessage::ShutdownAll).unwrap();
        }

        drop(shutdown_complete_tx);
        info!("Waiting for shutdown completion signals from subsystems...");
        let shutdown_timeout = tokio::time::Duration::from_secs(5);
        tokio::select! {
            _ = shutdown_complete_rx.recv() => {
                info!("All subsystems reported shutdown complete.");
            }
            _ = tokio::time::sleep(shutdown_timeout) => {
                warn!("Graceful shutdown timed out after {shutdown_timeout:?} — forcing shutdown.");
                task_manager.abort_all().await;
            }
        }
        info!("Joining remaining tasks...");
        task_manager.join_all().await;
        info!("TranslatorSv2 shutdown complete.");
    }

    fn spawn_quote_sweeper(&self, task_manager: &Arc<TaskManager>, wallet: Arc<Wallet>) {
        let locking_privkey = self.config.wallet.locking_privkey.clone();

        if locking_privkey.is_none() {
            warn!("Quote sweeper running without locking_privkey; minted tokens cannot be signed");
        }

        task_manager.spawn(async move {
            let mut loop_count: u64 = 0;
            loop {
                loop_count += 1;
                info!("🕐 Quote sweeper loop #{} starting", loop_count);

                debug!("📞 About to call process_stored_quotes");
                match Self::process_stored_quotes(&wallet, locking_privkey.as_deref()).await {
                    Ok(_minted_amount) => {
                        if let Ok(balance) = wallet.total_balance().await {
                            info!("💰 Wallet balance after sweep: {} ehash", balance);
                        }
                    }
                    Err(e) => {
                        error!("❌ Quote processing failed: {}", e);
                    }
                }

                debug!("😴 Quote sweeper sleeping for 15 seconds...");
                tokio::time::sleep(Duration::from_secs(15)).await;
                debug!("⏰ Quote sweeper woke up from sleep");
            }
        });
    }

    async fn process_stored_quotes(
        wallet: &Arc<Wallet>,
        locking_privkey: Option<&str>,
    ) -> Result<u64> {
        let pending_quotes = match wallet.get_unpaid_mint_quotes().await {
            Ok(quotes) => quotes,
            Err(e) => {
                error!("Failed to fetch pending quotes from wallet: {}", e);
                return Ok(0);
            }
        };

        match wallet.total_balance().await {
            Ok(balance) => {
                info!("💰 Current wallet balance: {} ehash", balance);
            }
            Err(e) => {
                error!("Failed to get wallet balance: {}", e);
            }
        }

        info!(
            "📋 Found {} pending quotes with mintable amount",
            pending_quotes.len()
        );

        let quote_ids: Vec<String> = pending_quotes.iter().map(|q| q.id.clone()).collect();

        if quote_ids.is_empty() {
            return Ok(0);
        }

        let mut total_minted = 0u64;

        for quote_id in quote_ids.iter() {
            match Self::fetch_and_add_quote_to_wallet(wallet, quote_id).await {
                Ok(_) => match wallet.mint_quote_state_mining_share(quote_id).await {
                    Ok(quote_response) => {
                        if quote_response.is_fully_issued() {
                            debug!(
                                "Quote {} is already fully issued ({}), skipping",
                                quote_id, quote_response.amount_issued
                            );
                            continue;
                        }

                        let amount = quote_response.amount.unwrap_or(Amount::ZERO);
                        let keyset_id = quote_response.keyset_id;

                        let secret_key = match locking_privkey {
                            Some(privkey_hex) => match hex::decode(privkey_hex) {
                                Ok(privkey_bytes) => match SecretKey::from_slice(&privkey_bytes) {
                                    Ok(sk) => sk,
                                    Err(e) => {
                                        error!(
                                            "Invalid secret key format for quote {}: {}",
                                            quote_id, e
                                        );
                                        continue;
                                    }
                                },
                                Err(e) => {
                                    error!(
                                        "Failed to decode secret key hex for quote {}: {}",
                                        quote_id, e
                                    );
                                    continue;
                                }
                            },
                            None => {
                                error!(
                                    "Secret key is required for mining share minting (quote {})",
                                    quote_id
                                );
                                continue;
                            }
                        };

                        match wallet
                            .mint_mining_share(quote_id, amount, keyset_id, secret_key)
                            .await
                        {
                            Ok(proofs) => {
                                let amount: u64 = proofs.iter().map(|p| u64::from(p.amount)).sum();
                                total_minted += amount;
                            }
                            Err(e) => {
                                warn!("Failed to mint quote {}: {}", quote_id, e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get quote details for {}: {}", quote_id, e);
                    }
                },
                Err(e) => {
                    warn!("Failed to fetch quote {} details: {}", quote_id, e);
                }
            }
        }

        if total_minted > 0 {
            info!(
                "Minted {} ehash from {} quotes",
                total_minted,
                quote_ids.len()
            );
        } else {
            warn!("😞 No tokens were minted from any quotes");
        }

        Ok(total_minted)
    }

    async fn fetch_and_add_quote_to_wallet(wallet: &Arc<Wallet>, quote_id: &str) -> Result<()> {
        debug!("🔍 Fetching quote {} from mint", quote_id);

        let quote = wallet
            .mint_quote_state_mining_share(quote_id)
            .await
            .with_context(|| format!("Failed to fetch quote {} from mint", quote_id))?;

        debug!(
            "💾 Quote {} fetched and added to wallet (state: {:?})",
            quote_id, quote.state
        );
        Ok(())
    }
}
