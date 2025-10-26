//! Pool messages adapter crate
//!
//! This crate provides a simple re-export of `PoolMessages` type that integrates
//! with the SV2 protocol implementation. It serves as a compatibility layer to ensure
//! that the mint service and other components can work with pool messages using standard
//! SV2 message types.

#![no_std]
extern crate alloc;

/// Re-export PoolMessages for mint crate compatibility
/// Provides the PoolMessages type that mint role expects
pub use roles_logic_sv2::parsers_sv2::PoolMessages;

/// Re-export Mining enum for extension message handling
pub use roles_logic_sv2::parsers_sv2::Mining;

/// Re-export common message types
pub use roles_logic_sv2::parsers_sv2::{CommonMessages, TemplateDistribution, JobDeclaration};

/// Re-export AnyMessage for flexible message handling
pub use roles_logic_sv2::parsers_sv2::AnyMessage;
