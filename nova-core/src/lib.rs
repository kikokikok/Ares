//! Nova Core Engine
//!
//! A high-performance modular game engine designed for real-time applications
//! requiring sophisticated resource management and extensible plugin capabilities.

pub mod config;
pub mod engine;
pub mod error;
pub mod events;
pub mod plugins;
pub mod protocol;
pub mod resources;
pub mod threading;

pub use config::EngineConfig;
pub use engine::NovaEngine;
pub use error::{NovaError, NovaResult};
pub use events::{Event, EventSystem};
pub use plugins::{Plugin, PluginManager};
pub use protocol::{
    ClientCapabilities, Disconnect, DisconnectReason, Error, ErrorCode, GameState, Handshake,
    HandshakeResponse, Heartbeat, HeartbeatResponse, NovaProtocol, Packet, PacketType,
    PlayerAction, ProtoSerialize, ResourceRequest, ResourceResponse, ServerSettings,
};
pub use resources::{Resource, ResourceManager};
pub use threading::ThreadEngine;

/// Nova Core Engine version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize the Nova Core Engine with default settings
pub async fn init() -> NovaResult<NovaEngine> {
    NovaEngine::new().await
}

/// Initialize the Nova Core Engine with custom configuration
pub async fn init_with_config(config: EngineConfig) -> NovaResult<NovaEngine> {
    NovaEngine::with_config(config).await
}
