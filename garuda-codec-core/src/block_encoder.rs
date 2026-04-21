use crate::vector;
use raptorq::{
    EncodingPacket, ObjectTransmissionInformation, SourceBlockEncoder, SourceBlockEncodingPlan,
};
use std::error;
use std::fmt;
use std::fmt::Debug;
use std::sync::mpsc;

pub struct Block {
    pub id: u8,
    pub data: Vec<u8>,
}

impl Clone for Block {
    fn clone(&self) -> Self {
        Block {
            id: self.id,
            data: self.data.clone(),
        }
    }
}

pub struct BlockEncoder {
    block_size: usize,
    repair_packets_per_block: u32,
    config: ObjectTransmissionInformation,
    encoding_plan: SourceBlockEncodingPlan,
}

impl BlockEncoder {
    pub fn new(
        block_size: usize,
        max_packet_size: u16,
        repair_packets_per_block: u32,
    ) -> Result<BlockEncoder, BlockEncoderError> {
        let config = get_configuration(block_size, max_packet_size)?;

        let symbol_count = block_size / config.symbol_size() as usize;
        let symbol_count = match u16::try_from(symbol_count) {
            Ok(v) => v,
            Err(_) => {
                return Err(BlockEncoderError::InvalidConfiguration(
                    "Cannot safely cast symbol_count to u16".into(),
                ));
            }
        };

        let encoding_plan = SourceBlockEncodingPlan::generate(symbol_count);

        Ok(BlockEncoder {
            block_size,
            repair_packets_per_block,
            config,
            encoding_plan,
        })
    }

    pub fn encode(
        &mut self,
        block: &Block,
    ) -> Result<impl Iterator<Item = EncodingPacket>, BlockEncoderError> {
        if block.data.len() != self.block_size {
            return Err(BlockEncoderError::UnexpectedBlockSize);
        }

        let block_encoder = SourceBlockEncoder::with_encoding_plan(
            block.id,
            &self.config,
            &block.data,
            &self.encoding_plan,
        );

        let source_packets = block_encoder.source_packets();
        let repair_packets = block_encoder.repair_packets(0, self.repair_packets_per_block);
        let packets = vector::interleave(source_packets, repair_packets);

        Ok(packets)
    }

    pub fn encode_stream(
        &mut self,
        input: impl IntoIterator<Item = Block>,
        output: &mpsc::SyncSender<EncodingPacket>,
    ) -> Result<(), BlockEncoderError> {
        for block in input {
            for packet in self.encode(&block)? {
                output.send(packet)?;
            }
        }

        Ok(())
    }
}

pub fn get_configuration(
    block_size: usize,
    max_packet_size: u16,
) -> Result<ObjectTransmissionInformation, ConfigurationError> {
    let config = ObjectTransmissionInformation::with_defaults(block_size as u64, max_packet_size);
    let symbol_size = config.symbol_size() as usize;

    if !block_size.is_multiple_of(symbol_size) {
        return Err(ConfigurationError::InvalidBlockSize(
            block_size,
            symbol_size,
        ));
    }

    Ok(config)
}

#[derive(Debug)]
pub enum ConfigurationError {
    InvalidBlockSize(usize, usize),
}

impl error::Error for ConfigurationError {}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBlockSize(block_size, symbol_size) => {
                write!(
                    f,
                    "Block size {block_size} is not a multiple of symbol size {symbol_size}"
                )
            }
        }
    }
}

#[derive(Debug)]
pub enum BlockEncoderError {
    UnexpectedBlockSize,
    InvalidConfiguration(String),
    Send(mpsc::SendError<EncodingPacket>),
}

impl fmt::Display for BlockEncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedBlockSize => {
                write!(f, "Block size does not match encoder configuration")
            }
            Self::InvalidConfiguration(s) => {
                write!(f, "{s}")
            }
            Self::Send(e) => {
                write!(f, "{e}")
            }
        }
    }
}

impl From<ConfigurationError> for BlockEncoderError {
    fn from(err: ConfigurationError) -> BlockEncoderError {
        BlockEncoderError::InvalidConfiguration(err.to_string())
    }
}

impl From<mpsc::SendError<EncodingPacket>> for BlockEncoderError {
    fn from(err: mpsc::SendError<EncodingPacket>) -> BlockEncoderError {
        BlockEncoderError::Send(err)
    }
}

pub fn compute_repair_packets_per_block(
    block_size: usize,
    max_packet_size: u16,
    redundancy_factor: u32,
) -> Result<u32, Box<dyn error::Error>> {
    let config = get_configuration(block_size, max_packet_size)?;
    let num_source_symbols = u32::try_from(block_size / config.symbol_size() as usize)?;

    Ok((num_source_symbols * redundancy_factor).div_ceil(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::get_random_vec;

    #[test]
    fn test_encode() {
        const BLOCK_SIZE: usize = 1496 * 192; // 280.5 KiB
        const MAX_PACKET_SIZE: u16 = 1500;
        const NUM_REPAIR_PACKETS: u32 = 15;

        let source_block = Block {
            id: 1,
            data: get_random_vec(BLOCK_SIZE).unwrap(),
        };

        let mut encoder =
            BlockEncoder::new(BLOCK_SIZE, MAX_PACKET_SIZE, NUM_REPAIR_PACKETS).unwrap();

        let packets: Vec<_> = encoder.encode(&source_block).unwrap().collect();

        let expected_num_packets =
            (BLOCK_SIZE as u64 / encoder.config.symbol_size() as u64) + NUM_REPAIR_PACKETS as u64;
        assert_eq!(packets.len() as u64, expected_num_packets);

        let another_block = Block {
            id: 2,
            data: get_random_vec(BLOCK_SIZE).unwrap(),
        };

        let _ = encoder.encode(&another_block).unwrap();
    }
}
