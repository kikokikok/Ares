//! High-performance network protocol implementation using Protocol Buffers
//! 
//! This module provides blazing fast, polyglot serialization for the Nova Core Engine
//! optimized for wire transfer performance and minimal memory consumption.

use std::io::{Cursor, Read};
use prost::Message;
use bytes::{Bytes, BytesMut, Buf, BufMut};
use crate::error::{NovaError, NovaResult};

// Include generated protobuf code
pub mod protocol {
    include!(concat!(env!("OUT_DIR"), "/nova.protocol.rs"));
}

pub mod resources {
    include!(concat!(env!("OUT_DIR"), "/nova.resources.rs"));
}

pub use protocol::*;
pub use resources::*;

/// High-performance protocol codec for Nova messages
#[derive(Debug, Clone)]
pub struct NovaProtocol {
    max_message_size: usize,
    compression_enabled: bool,
}

impl Default for NovaProtocol {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB default
            compression_enabled: false,
        }
    }
}

impl NovaProtocol {
    /// Create a new protocol codec
    pub fn new(max_message_size: usize, compression_enabled: bool) -> Self {
        Self {
            max_message_size,
            compression_enabled,
        }
    }
    
    /// Encode a message to bytes with maximum performance
    pub fn encode<T: Message>(&self, message: &T) -> NovaResult<Bytes> {
        let encoded_len = message.encoded_len();
        if encoded_len > self.max_message_size {
            return Err(NovaError::Generic(anyhow::anyhow!(
                "Message too large: {} > {}", encoded_len, self.max_message_size
            )));
        }
        
        let mut buf = BytesMut::with_capacity(encoded_len);
        message.encode(&mut buf)
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Failed to encode message: {}", e)))?;
        
        Ok(buf.freeze())
    }
    
    /// Decode bytes to a message with maximum performance
    pub fn decode<T: Message + Default>(&self, data: &[u8]) -> NovaResult<T> {
        if data.len() > self.max_message_size {
            return Err(NovaError::Generic(anyhow::anyhow!(
                "Message too large: {} > {}", data.len(), self.max_message_size
            )));
        }
        
        T::decode(data)
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Failed to decode message: {}", e)))
    }
    
    /// Encode a packet with header information
    pub fn encode_packet(&self, packet: &protocol::Packet) -> NovaResult<Bytes> {
        // Add 4-byte length prefix for framing
        let encoded = self.encode(packet)?;
        let total_len = encoded.len() + 4;
        
        let mut buf = BytesMut::with_capacity(total_len);
        buf.put_u32_le(encoded.len() as u32);
        buf.extend_from_slice(&encoded);
        
        Ok(buf.freeze())
    }
    
    /// Decode a packet from framed data
    pub fn decode_packet(&self, data: &mut Cursor<&[u8]>) -> NovaResult<Option<protocol::Packet>> {
        if data.remaining() < 4 {
            return Ok(None); // Not enough data for length prefix
        }
        
        let len = data.get_u32_le() as usize;
        if len > self.max_message_size {
            return Err(NovaError::Generic(anyhow::anyhow!(
                "Packet too large: {} > {}", len, self.max_message_size
            )));
        }
        
        if data.remaining() < len {
            return Ok(None); // Not enough data for full packet
        }
        
        // Read packet data into a separate buffer to avoid borrowing issues
        let mut packet_data = vec![0u8; len];
        data.read_exact(&mut packet_data)
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Failed to read packet data: {}", e)))?;
        
        let packet = self.decode(&packet_data)?;
        Ok(Some(packet))
    }
    
    /// Create a heartbeat packet
    pub fn create_heartbeat(timestamp: u64, sequence: u32) -> protocol::Packet {
        let heartbeat = protocol::Heartbeat {
            timestamp,
            sequence,
        };
        
        let mut payload = BytesMut::new();
        heartbeat.encode(&mut payload).unwrap();
        
        protocol::Packet {
            id: uuid::Uuid::new_v4().as_u128() as u64,
            timestamp,
            r#type: protocol::PacketType::PacketTypeHeartbeat as i32,
            payload: payload.freeze(),
            compression: None,
        }
    }
    
    /// Create a handshake packet
    pub fn create_handshake(
        client_version: String,
        player_name: String,
        capabilities: protocol::ClientCapabilities,
    ) -> protocol::Packet {
        let handshake = protocol::Handshake {
            client_version,
            player_name,
            capabilities: Some(capabilities),
        };
        
        let mut payload = BytesMut::new();
        handshake.encode(&mut payload).unwrap();
        
        protocol::Packet {
            id: uuid::Uuid::new_v4().as_u128() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            r#type: protocol::PacketType::PacketTypeHandshake as i32,
            payload: payload.freeze(),
            compression: None,
        }
    }
    
    /// Create an error packet
    pub fn create_error(code: protocol::ErrorCode, message: String) -> protocol::Packet {
        let error = protocol::Error {
            code: code as i32,
            message,
            details: None,
        };
        
        let mut payload = BytesMut::new();
        error.encode(&mut payload).unwrap();
        
        protocol::Packet {
            id: uuid::Uuid::new_v4().as_u128() as u64,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            r#type: protocol::PacketType::PacketTypeError as i32,
            payload: payload.freeze(),
            compression: None,
        }
    }
    
    /// Extract typed message from packet payload
    pub fn extract_payload<T: Message + Default>(&self, packet: &protocol::Packet) -> NovaResult<T> {
        self.decode(&packet.payload)
    }
}

/// Performance-optimized resource serialization
pub trait ProtoSerialize {
    type Proto: Message + Default;
    
    /// Convert to protobuf message
    fn to_proto(&self) -> Self::Proto;
    
    /// Create from protobuf message
    fn from_proto(proto: Self::Proto) -> NovaResult<Self>
    where
        Self: Sized;
    
    /// Serialize to bytes using protobuf
    fn serialize_proto(&self) -> NovaResult<Vec<u8>> {
        let proto = self.to_proto();
        let mut buf = Vec::with_capacity(proto.encoded_len());
        proto.encode(&mut buf)
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Serialization failed: {}", e)))?;
        Ok(buf)
    }
    
    /// Deserialize from bytes using protobuf
    fn deserialize_proto(data: &[u8]) -> NovaResult<Self>
    where
        Self: Sized,
    {
        let proto = Self::Proto::decode(data)
            .map_err(|e| NovaError::Generic(anyhow::anyhow!("Deserialization failed: {}", e)))?;
        Self::from_proto(proto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_protocol_codec() {
        let protocol = NovaProtocol::default();
        
        // Test heartbeat encoding/decoding
        let heartbeat = protocol::Heartbeat {
            timestamp: 12345,
            sequence: 1,
        };
        
        let encoded = protocol.encode(&heartbeat).unwrap();
        let decoded: protocol::Heartbeat = protocol.decode(&encoded).unwrap();
        
        assert_eq!(heartbeat, decoded);
    }
    
    #[test]
    fn test_packet_framing() {
        let protocol = NovaProtocol::default();
        let packet = NovaProtocol::create_heartbeat(12345, 1);
        
        let encoded = protocol.encode_packet(&packet).unwrap();
        let mut cursor = Cursor::new(encoded.as_ref());
        let decoded = protocol.decode_packet(&mut cursor).unwrap().unwrap();
        
        assert_eq!(packet.timestamp, decoded.timestamp);
        assert_eq!(packet.r#type, decoded.r#type);
    }
    
    #[test]
    fn test_message_size_limits() {
        let protocol = NovaProtocol::new(100, false); // Small limit for testing
        
        let large_packet = protocol::Packet {
            id: 1,
            timestamp: 12345,
            r#type: protocol::PacketType::PacketTypeHeartbeat as i32,
            payload: vec![0u8; 200].into(), // Larger than limit
            compression: None,
        };
        
        assert!(protocol.encode(&large_packet).is_err());
    }
}