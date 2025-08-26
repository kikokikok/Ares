use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use log::{info, warn, error};
use nova_core::{NovaEngine, EngineConfig, NovaResult, NovaError};

/// Client configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub client: ClientSettings,
    pub engine: EngineConfig,
    pub networking: NetworkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSettings {
    pub name: String,
    pub version: String,
    pub auto_connect: bool,
    pub reconnect_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub server_address: String,
    pub connection_timeout: u64,
    pub heartbeat_interval: u64,
    pub max_packet_size: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            client: ClientSettings {
                name: "Nova Client".to_string(),
                version: "1.0.0".to_string(),
                auto_connect: true,
                reconnect_attempts: 3,
            },
            engine: EngineConfig::default(),
            networking: NetworkConfig {
                server_address: "127.0.0.1:8080".to_string(),
                connection_timeout: 10,
                heartbeat_interval: 30,
                max_packet_size: 1024 * 1024, // 1MB
            },
        }
    }
}

/// Connection status
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed(String),
}

/// Nova Client for connecting to Nova Servers
pub struct NovaClient {
    config: ClientConfig,
    engine: Arc<NovaEngine>,
    connection: Arc<Mutex<Option<TcpStream>>>,
    status: Arc<Mutex<ConnectionStatus>>,
}

impl NovaClient {
    /// Create a new Nova Client
    pub async fn new(config: ClientConfig) -> NovaResult<Self> {
        info!("Initializing Nova Client: {}", config.client.name);
        
        let engine = Arc::new(NovaEngine::with_config(config.engine.clone()).await?);
        
        let client = Self {
            config,
            engine,
            connection: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(ConnectionStatus::Disconnected)),
        };
        
        info!("Nova Client initialized successfully");
        Ok(client)
    }
    
    /// Create client from configuration file
    pub async fn from_config(config_path: PathBuf, server_address: String) -> NovaResult<Self> {
        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| NovaError::config(format!("Failed to read client config: {}", e)))?;
            toml::from_str(&content)
                .map_err(|e| NovaError::config(format!("Failed to parse client config: {}", e)))?
        } else {
            ClientConfig::default()
        };
        
        config.networking.server_address = server_address;
        Self::new(config).await
    }
    
    /// Connect to the server
    pub async fn connect(&self) -> NovaResult<()> {
        info!("Connecting to server: {}", self.config.networking.server_address);
        
        *self.status.lock().await = ConnectionStatus::Connecting;
        
        match self.attempt_connection().await {
            Ok(stream) => {
                *self.connection.lock().await = Some(stream);
                *self.status.lock().await = ConnectionStatus::Connected;
                info!("Connected to server successfully");
                
                // Start the client main loop
                self.run_client_loop().await?;
                
                Ok(())
            }
            Err(e) => {
                *self.status.lock().await = ConnectionStatus::Failed(e.to_string());
                error!("Failed to connect to server: {}", e);
                
                if self.config.client.auto_connect {
                    self.handle_reconnection().await?;
                }
                
                Err(e)
            }
        }
    }
    
    /// Attempt to establish connection
    async fn attempt_connection(&self) -> NovaResult<TcpStream> {
        let timeout = Duration::from_secs(self.config.networking.connection_timeout);
        
        match tokio::time::timeout(timeout, TcpStream::connect(&self.config.networking.server_address)).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(e)) => Err(NovaError::Generic(anyhow::anyhow!("Connection failed: {}", e))),
            Err(_) => Err(NovaError::Generic(anyhow::anyhow!("Connection timeout"))),
        }
    }
    
    /// Handle reconnection logic
    async fn handle_reconnection(&self) -> NovaResult<()> {
        let mut attempts = 0;
        let max_attempts = self.config.client.reconnect_attempts;
        
        while attempts < max_attempts {
            attempts += 1;
            *self.status.lock().await = ConnectionStatus::Reconnecting;
            
            warn!("Reconnection attempt {} of {}", attempts, max_attempts);
            
            // Wait before reconnecting
            tokio::time::sleep(Duration::from_secs(2 * attempts as u64)).await;
            
            match self.attempt_connection().await {
                Ok(stream) => {
                    *self.connection.lock().await = Some(stream);
                    *self.status.lock().await = ConnectionStatus::Connected;
                    info!("Reconnected to server successfully");
                    return Ok(());
                }
                Err(e) => {
                    warn!("Reconnection attempt {} failed: {}", attempts, e);
                }
            }
        }
        
        *self.status.lock().await = ConnectionStatus::Failed("Max reconnection attempts exceeded".to_string());
        Err(NovaError::Generic(anyhow::anyhow!("Failed to reconnect after {} attempts", max_attempts)))
    }
    
    /// Run the main client loop
    async fn run_client_loop(&self) -> NovaResult<()> {
        info!("Starting client main loop");
        
        // Start the engine
        let engine = self.engine.clone();
        let engine_task = tokio::spawn(async move {
            if let Err(e) = engine.run().await {
                error!("Engine error: {}", e);
            }
        });
        
        // Start heartbeat task
        let heartbeat_task = self.start_heartbeat();
        
        // Start message handling task
        let message_task = self.start_message_handler();
        
        // Wait for any task to complete or fail
        tokio::select! {
            result = engine_task => {
                if let Err(e) = result {
                    error!("Engine task failed: {}", e);
                }
            }
            result = heartbeat_task => {
                if let Err(e) = result {
                    error!("Heartbeat task failed: {}", e);
                }
            }
            result = message_task => {
                if let Err(e) = result {
                    error!("Message handler task failed: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Start heartbeat task
    async fn start_heartbeat(&self) -> NovaResult<()> {
        let interval = Duration::from_secs(self.config.networking.heartbeat_interval);
        let mut heartbeat_timer = tokio::time::interval(interval);
        
        loop {
            heartbeat_timer.tick().await;
            
            let status = self.status.lock().await.clone();
            if status != ConnectionStatus::Connected {
                break;
            }
            
            // Send heartbeat message
            if let Err(e) = self.send_heartbeat().await {
                warn!("Heartbeat failed: {}", e);
                *self.status.lock().await = ConnectionStatus::Failed("Heartbeat failed".to_string());
                break;
            }
        }
        
        Ok(())
    }
    
    /// Send heartbeat message to server
    async fn send_heartbeat(&self) -> NovaResult<()> {
        // This would send a heartbeat packet to the server
        // For now, we'll just log it
        log::trace!("Sending heartbeat to server");
        Ok(())
    }
    
    /// Start message handler task
    async fn start_message_handler(&self) -> NovaResult<()> {
        // This would handle incoming messages from the server
        // For now, we'll just simulate message processing
        loop {
            let status = self.status.lock().await.clone();
            if status != ConnectionStatus::Connected {
                break;
            }
            
            // Simulate message processing
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        Ok(())
    }
    
    /// Disconnect from server
    pub async fn disconnect(&self) -> NovaResult<()> {
        info!("Disconnecting from server");
        
        *self.connection.lock().await = None;
        *self.status.lock().await = ConnectionStatus::Disconnected;
        
        // Shutdown the engine
        self.engine.shutdown().await?;
        
        info!("Disconnected from server");
        Ok(())
    }
    
    /// Get connection status
    pub async fn status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }
    
    /// Get client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }
    
    /// Get engine instance
    pub fn engine(&self) -> &Arc<NovaEngine> {
        &self.engine
    }
    
    /// Send data to server
    pub async fn send_data(&self, data: &[u8]) -> NovaResult<()> {
        if data.len() > self.config.networking.max_packet_size {
            return Err(NovaError::Generic(anyhow::anyhow!("Packet too large")));
        }
        
        let status = self.status.lock().await.clone();
        if status != ConnectionStatus::Connected {
            return Err(NovaError::Generic(anyhow::anyhow!("Not connected to server")));
        }
        
        // This would actually send data over the connection
        log::trace!("Sending {} bytes to server", data.len());
        Ok(())
    }
}

impl Drop for NovaClient {
    fn drop(&mut self) {
        // Note: Can't call async methods in Drop
        info!("Nova Client dropped");
    }
}