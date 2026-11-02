//! Ferrydock core engine: a multi-provider, OpenAI-compatible proxy with
//! failover, circuit breaking, bounded concurrency, and WorkBuddy sync.

pub mod config;
pub mod error;
pub mod metrics;
pub mod providers;
pub mod router;
pub mod runtime;
pub mod server;
pub mod sync;

pub use config::{Config, ProviderConfig, ProviderKind};
pub use error::{Error, Result};
pub use router::{ChatOutcome, Engine, ModelEntry};
