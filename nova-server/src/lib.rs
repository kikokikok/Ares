use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::io::Cursor;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::{Deserialize, Serialize};
use dashmap::DashMap;
use uuid::Uuid;
use log::{info, warn, error, debug};
use nova_core::{NovaEngine, EngineConfig, NovaResult, NovaError, NovaProtocol};
use prost::Message;
use bytes::{Bytes, BytesMut, Buf, BufMut};

// Include generated protobuf code for networking
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/nova.protocol.rs"));
}

pub use protocol::*;

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub engine: EngineConfig,
    pub networking: NetworkConfig,
    pub game: GameSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    pub name: String,
    pub version: String,
    pub max_clients: usize,
    pub tick_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub bind_address: String,
    pub port: u16,
    pub connection_timeout: u64,
    pub max_packet_size: usize,
    pub compression: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSettings {
    pub world_size: u32,
    pub max_entities: usize,
    pub physics_enabled: bool,
    pub save_interval: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: ServerSettings {
                name: "Nova Server".to_string(),
                version: "1.0.0".to_string(),
                max_clients: 100,
                tick_rate: 60,
            },
            engine: EngineConfig::default(),
            networking: NetworkConfig {
                bind_address: "0.0.0.0".to_string(),
                port: 8080,
                connection_timeout: 30,
                max_packet_size: 1024 * 1024, // 1MB
                compression: true,
            },
            game: GameSettings {
                world_size: 1000,
                max_entities: 10000,
                physics_enabled: true,
                save_interval: 300, // 5 minutes
            },
        }
    }
}

/// Client connection information with high-performance networking
#[derive(Debug)]
pub struct ClientConnection {
    pub id: Uuid,
    pub addr: SocketAddr,
    pub stream: Arc<Mutex<TcpStream>>,
    pub connected_at: Instant,
    pub last_heartbeat: Instant,
    pub player_name: Option<String>,
    pub protocol: NovaProtocol,
    pub session_id: String,
    pub heartbeat_sequence: u32,
}

/// Server statistics
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub connected_clients: usize,
    pub total_connections: u64,
    pub uptime: Duration,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub ticks_processed: u64,
    pub avg_tick_time_ms: f64,
}

/// Nova Server for hosting multiplayer game sessions with high-performance networking
pub struct NovaServer {
    config: ServerConfig,
    engine: Arc<NovaEngine>,
    clients: Arc<DashMap<Uuid, ClientConnection>>,
    listener: Option<TcpListener>,
    stats: Arc<Mutex<ServerStats>>,
    start_time: Instant,
    protocol: NovaProtocol,
}

impl NovaServer {
    /// Create a new Nova Server
    pub async fn new(config: ServerConfig) -> NovaResult<Self> {
        info!("Initializing Nova Server: {}", config.server.name);
        
        let engine = Arc::new(NovaEngine::with_config(config.engine.clone()).await?);
        
        let server = Self {
            config,
            engine,
            clients: Arc::new(DashMap::new()),
            listener: None,
            stats: Arc::new(Mutex::new(ServerStats {
                connected_clients: 0,
                total_connections: 0,
                uptime: Duration::new(0, 0),
                bytes_sent: 0,
                bytes_received: 0,
                ticks_processed: 0,
                avg_tick_time_ms: 0.0,
            })),
            start_time: Instant::now(),
            protocol: NovaProtocol::new(1024 * 1024, true), // 1MB max, compression enabled
        };
        
        info!("Nova Server initialized successfully");
        Ok(server)
    }
    
    /// Create server from configuration file and port
    pub async fn from_config(config_path: PathBuf, port: u16) -> NovaResult<Self> {
        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .map_err(|e| NovaError::config(format!("Failed to read server config: {}", e)))?;
            toml::from_str(&content)
                .map_err(|e| NovaError::config(format!("Failed to parse server config: {}", e)))?
        } else {
            ServerConfig::default()
        };
        
        config.networking.port = port;
        Self::new(config).await
    }
    
    /// Start the server
    pub async fn start(&self) -> NovaResult<()> {
        info!("Starting Nova Server on {}:{}", 
               self.config.networking.bind_address, self.config.networking.port);
        
        // Bind to the specified address
        let addr = format!("{}:{}", self.config.networking.bind_address, self.config.networking.port);
        let listener = TcpListener::bind(&addr).await
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Failed to bind to {}: {}", addr, e)))?;
        
        info!("Server listening on {}", addr);
        
        // Start the engine
        let engine = self.engine.clone();
        let engine_task = tokio::spawn(async move {
            if let Err(e) = engine.run().await {
                error!("Engine error: {}", e);
            }
        });
        
        // Start the server main loop
        let server_task = self.run_server_loop(listener);
        
        // Start the game tick loop
        let tick_task = self.start_tick_loop();
        
        // Start statistics logging
        let stats_task = self.start_stats_logger();
        
        // Wait for any task to complete or fail
        tokio::select! {
            result = engine_task => {
                if let Err(e) = result {
                    error!("Engine task failed: {}", e);
                }
            }
            result = server_task => {
                if let Err(e) = result {
                    error!("Server task failed: {}", e);
                }
            }
            result = tick_task => {
                if let Err(e) = result {
                    error!("Tick task failed: {}", e);
                }
            }
            result = stats_task => {
                if let Err(e) = result {
                    error!("Stats task failed: {}", e);
                }
            }
        }
        
        Ok(())
    }
    
    /// Run the main server loop
    async fn run_server_loop(&self, listener: TcpListener) -> NovaResult<()> {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    if self.clients.len() >= self.config.server.max_clients {
                        warn!("Connection from {} rejected: server full", addr);
                        continue;
                    }
                    
                    let client_id = Uuid::new_v4();
                    let session_id = format!("{}-{}", client_id, Instant::now().elapsed().as_millis());
                    let connection = ClientConnection {
                        id: client_id,
                        addr,
                        stream: Arc::new(Mutex::new(stream)),
                        connected_at: Instant::now(),
                        last_heartbeat: Instant::now(),
                        player_name: None,
                        protocol: NovaProtocol::new(1024 * 1024, true),
                        session_id,
                        heartbeat_sequence: 0,
                    };
                    
                    self.clients.insert(client_id, connection);
                    
                    // Update statistics
                    {
                        let mut stats = self.stats.lock().await;
                        stats.connected_clients = self.clients.len();
                        stats.total_connections += 1;
                    }
                    
                    info!("Client {} connected from {}", client_id, addr);
                    
                    // Start client handler
                    let clients = self.clients.clone();
                    let config = self.config.clone();
                    tokio::spawn(async move {
                        Self::handle_client(client_id, clients, config).await;
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }
    
    /// Handle individual client connections
    async fn handle_client(
        client_id: Uuid,
        clients: Arc<DashMap<Uuid, ClientConnection>>,
        config: ServerConfig,
    ) {
        debug!("Starting client handler for {}", client_id);
        
        // Client message processing loop
        loop {
            // Check if client is still connected
            if !clients.contains_key(&client_id) {
                break;
            }
            
            // Check for client timeout
            if let Some(client) = clients.get(&client_id) {
                let timeout_duration = Duration::from_secs(config.networking.connection_timeout);
                if client.last_heartbeat.elapsed() > timeout_duration {
                    warn!("Client {} timed out", client_id);
                    break;
                }
            }
            
            // Process client messages (simplified)
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        
        // Clean up client connection
        if let Some((_, client)) = clients.remove(&client_id) {
            info!("Client {} ({}) disconnected", client_id, client.addr);
        }
    }
    
    /// Start the game tick loop
    async fn start_tick_loop(&self) -> NovaResult<()> {
        let tick_interval = Duration::from_millis(1000 / self.config.server.tick_rate as u64);
        let mut tick_timer = tokio::time::interval(tick_interval);
        let mut tick_count = 0u64;
        
        info!("Starting game tick loop ({}Hz)", self.config.server.tick_rate);
        
        loop {
            tick_timer.tick().await;
            let tick_start = Instant::now();
            
            // Process game tick
            self.process_game_tick(tick_count).await?;
            
            let tick_duration = tick_start.elapsed();
            
            // Update statistics
            {
                let mut stats = self.stats.lock().await;
                stats.ticks_processed += 1;
                stats.avg_tick_time_ms = 
                    (stats.avg_tick_time_ms + tick_duration.as_millis() as f64) / 2.0;
            }
            
            tick_count += 1;
            
            // Log warning if tick took too long
            if tick_duration > tick_interval {
                warn!("Tick {} took {:.1}ms (target: {:.1}ms)", 
                      tick_count, tick_duration.as_millis(), tick_interval.as_millis());
            }
        }
    }
    
    /// Process a single game tick
    async fn process_game_tick(&self, tick_count: u64) -> NovaResult<()> {
        // Update connected clients
        {
            let mut stats = self.stats.lock().await;
            stats.connected_clients = self.clients.len();
            stats.uptime = self.start_time.elapsed();
        }
        
        // Perform periodic maintenance
        if tick_count % (self.config.server.tick_rate as u64 * 60) == 0 {
            self.perform_maintenance().await?;
        }
        
        // Auto-save game state
        if tick_count % (self.config.server.tick_rate as u64 * self.config.game.save_interval) == 0 {
            self.save_game_state().await?;
        }
        
        Ok(())
    }
    
    /// Perform server maintenance tasks
    async fn perform_maintenance(&self) -> NovaResult<()> {
        debug!("Performing server maintenance");
        
        // Clean up disconnected clients
        let mut to_remove = Vec::new();
        for entry in self.clients.iter() {
            let (id, client) = entry.pair();
            let timeout_duration = Duration::from_secs(self.config.networking.connection_timeout);
            if client.last_heartbeat.elapsed() > timeout_duration {
                to_remove.push(*id);
            }
        }
        
        for id in to_remove {
            self.clients.remove(&id);
            debug!("Removed timed out client: {}", id);
        }
        
        // Trigger engine garbage collection
        self.engine.resource_manager().garbage_collect()?;
        
        Ok(())
    }
    
    /// Save game state
    async fn save_game_state(&self) -> NovaResult<()> {
        debug!("Saving game state");
        // This would save the current game state to persistent storage
        // For now, we'll just log it
        Ok(())
    }
    
    /// Start statistics logging
    async fn start_stats_logger(&self) -> NovaResult<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            
            let stats = self.stats.lock().await.clone();
            info!(
                "Server Stats - Clients: {}, Uptime: {:.1}s, Ticks: {}, Avg Tick: {:.1}ms",
                stats.connected_clients,
                stats.uptime.as_secs_f64(),
                stats.ticks_processed,
                stats.avg_tick_time_ms
            );
        }
    }
    
    /// Broadcast protobuf message to all connected clients with blazing fast performance
    pub async fn broadcast_proto<T: Message>(&self, message: &T) -> NovaResult<()> {
        let encoded = self.protocol.encode(message)?;
        self.broadcast(&encoded).await
    }
    
    /// Send protobuf message to specific client with high performance
    pub async fn send_proto_to_client<T: Message>(&self, client_id: Uuid, message: &T) -> NovaResult<()> {
        let encoded = self.protocol.encode(message)?;
        self.send_to_client(client_id, &encoded).await
    }
    
    /// Send packet to specific client using the optimized protocol
    pub async fn send_packet_to_client(&self, client_id: Uuid, packet: &protocol::Packet) -> NovaResult<()> {
        let encoded = self.protocol.encode_packet(packet)?;
        
        if let Some(client) = self.clients.get(&client_id) {
            let mut stream = client.stream.lock().await;
            stream.write_all(&encoded).await
                .map_err(|e| NovaError::Generic(anyhow::anyhow!("Failed to send packet: {}", e)))?;
            
            // Update statistics
            let mut stats = self.stats.lock().await;
            stats.bytes_sent += encoded.len() as u64;
            
            debug!("Sent packet ({} bytes) to client {}", encoded.len(), client_id);
            Ok(())
        } else {
            Err(NovaError::Generic(anyhow::anyhow!("Client {} not found", client_id)))
        }
    }
    
    /// Broadcast packet to all connected clients
    pub async fn broadcast_packet(&self, packet: &protocol::Packet) -> NovaResult<()> {
        let encoded = self.protocol.encode_packet(packet)?;
        let mut total_sent = 0u64;
        
        for entry in self.clients.iter() {
            let (client_id, client) = entry.pair();
            match client.stream.lock().await.write_all(&encoded).await {
                Ok(_) => {
                    total_sent += encoded.len() as u64;
                    debug!("Broadcast packet to client {}", client_id);
                }
                Err(e) => {
                    warn!("Failed to send broadcast to client {}: {}", client_id, e);
                }
            }
        }
        
        // Update statistics
        {
            let mut stats = self.stats.lock().await;
            stats.bytes_sent += total_sent;
        }
        
        Ok(())
    }
    
    /// Send heartbeat to specific client
    pub async fn send_heartbeat_to_client(&self, client_id: Uuid) -> NovaResult<()> {
        if let Some(mut client) = self.clients.get_mut(&client_id) {
            client.heartbeat_sequence += 1;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            let packet = NovaProtocol::create_heartbeat(timestamp, client.heartbeat_sequence);
            self.send_packet_to_client(client_id, &packet).await
        } else {
            Err(NovaError::Generic(anyhow::anyhow!("Client {} not found", client_id)))
        }
    }
    
    /// Handle client handshake with protobuf protocol
    pub async fn handle_handshake(&self, client_id: Uuid, handshake: protocol::Handshake) -> NovaResult<()> {
        info!("Handling handshake from client {}: {}", client_id, handshake.player_name);
        
        // Update client information
        if let Some(mut client) = self.clients.get_mut(&client_id) {
            client.player_name = Some(handshake.player_name.clone());
        }
        
        // Create handshake response
        let response = protocol::HandshakeResponse {
            accepted: true,
            server_version: "1.0.0".to_string(),
            session_id: if let Some(client) = self.clients.get(&client_id) {
                client.session_id.clone()
            } else {
                return Err(NovaError::Generic(anyhow::anyhow!("Client not found")));
            },
            settings: Some(protocol::ServerSettings {
                tick_rate: self.config.server.tick_rate,
                max_players: self.config.server.max_clients as u32,
                compression_enabled: self.config.networking.compression,
                heartbeat_interval: 30, // seconds
            }),
            error_message: None,
        };
        
        let packet = protocol::Packet {
            id: uuid::Uuid::new_v4().as_u128() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            r#type: protocol::PacketType::PacketTypeHandshake as i32,
            payload: {
                let mut buf = BytesMut::new();
                response.encode(&mut buf).unwrap();
                buf.freeze()
            },
            compression: None,
        };
        
        self.send_packet_to_client(client_id, &packet).await
    }
    
    /// Broadcast message to all connected clients (legacy method for raw bytes)
    pub async fn broadcast(&self, data: &[u8]) -> NovaResult<()> {
        let mut bytes_sent = 0u64;
        
        for entry in self.clients.iter() {
            let (id, client) = entry.pair();
            match client.stream.lock().await.write_all(data).await {
                Ok(_) => {
                    bytes_sent += data.len() as u64;
                    debug!("Broadcasting {} bytes to client {}", data.len(), id);
                }
                Err(e) => {
                    warn!("Failed to broadcast to client {}: {}", id, e);
                }
            }
        }
        
        // Update statistics
        {
            let mut stats = self.stats.lock().await;
            stats.bytes_sent += bytes_sent;
        }
        
        Ok(())
    }
    
    /// Send message to specific client (legacy method for raw bytes)
    pub async fn send_to_client(&self, client_id: Uuid, data: &[u8]) -> NovaResult<()> {
        if let Some(client) = self.clients.get(&client_id) {
            match client.stream.lock().await.write_all(data).await {
                Ok(_) => {
                    debug!("Sending {} bytes to client {}", data.len(), client_id);
                    
                    // Update statistics
                    let mut stats = self.stats.lock().await;
                    stats.bytes_sent += data.len() as u64;
                    
                    Ok(())
                }
                Err(e) => {
                    Err(NovaError::Generic(anyhow::anyhow!("Failed to send to client: {}", e)))
                }
            }
        } else {
            Err(NovaError::Generic(anyhow::anyhow!("Client {} not found", client_id)))
        }
    }
    
    /// Get server statistics
    pub async fn get_stats(&self) -> ServerStats {
        let mut stats = self.stats.lock().await;
        stats.uptime = self.start_time.elapsed();
        stats.connected_clients = self.clients.len();
        stats.clone()
    }
    
    /// Get list of connected clients
    pub fn get_connected_clients(&self) -> Vec<Uuid> {
        self.clients.iter().map(|entry| *entry.key()).collect()
    }
    
    /// Shutdown the server
    pub async fn shutdown(&self) -> NovaResult<()> {
        info!("Shutting down Nova Server...");
        
        // Disconnect all clients
        for entry in self.clients.iter() {
            let (id, client) = entry.pair();
            info!("Disconnecting client {} ({})", id, client.addr);
        }
        self.clients.clear();
        
        // Shutdown the engine
        self.engine.shutdown().await?;
        
        let uptime = self.start_time.elapsed();
        info!("Nova Server shutdown complete (uptime: {:.1}s)", uptime.as_secs_f64());
        
        Ok(())
    }
    
    /// Get server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
    
    /// Get engine instance
    pub fn engine(&self) -> &Arc<NovaEngine> {
        &self.engine
    }
}

impl Drop for NovaServer {
    fn drop(&mut self) {
        info!("Nova Server dropped");
    }
}