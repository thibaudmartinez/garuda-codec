use crate::block_encoder;
use crate::block_encoder::{Block, BlockEncoder, ConfigurationError};
use raptorq::{EncodingPacket, ObjectTransmissionInformation, SourceBlockDecoder};
use std::sync::mpsc;
use std::{fmt, iter};

pub struct BlockDecoder {
    block_size: usize,
    config: ObjectTransmissionInformation,
    current_block_id: Option<u8>,
    current_decoder: SourceBlockDecoder,
    current_block_is_decoded: bool,
}

impl BlockDecoder {
    pub fn new(block_size: usize, max_packet_size: u16) -> Result<BlockDecoder, BlockDecoderError> {
        let config = block_encoder::get_configuration(block_size, max_packet_size)?;

        Ok(BlockDecoder {
            block_size,
            config,
            current_block_id: None,
            current_decoder: SourceBlockDecoder::new(0, &config, block_size as u64),
            current_block_is_decoded: false,
        })
    }

    pub fn decode(&mut self, packet: EncodingPacket) -> Option<Vec<u8>> {
        let received_block_id = packet.payload_id().source_block_number();

        if let Some(current_block_id) = self.current_block_id {
            // A block is currently being decoded.

            // Case 1. The packet received belongs to the current block.
            if received_block_id == current_block_id {
                if self.current_block_is_decoded {
                    // The block has already been decoded, ignore the packet.
                    return None;
                }

                return self.try_decode(packet);
            }

            // Case 2. The packet received belongs to a block different from the current one.
            //
            // Stop the decoding of the current packet and start decoding the block corresponding
            // to the received packet.
            self.reset_current_decoder(received_block_id);
            self.try_decode(packet)
        } else {
            // Case 3. No block is currently being decoded.
            self.reset_current_decoder(received_block_id);
            self.try_decode(packet)
        }
    }

    fn reset_current_decoder(&mut self, block_id: u8) {
        self.current_block_id = Some(block_id);
        self.current_block_is_decoded = false;
        self.current_decoder =
            SourceBlockDecoder::new(block_id, &self.config, self.block_size as u64);
    }

    fn try_decode(&mut self, packet: EncodingPacket) -> Option<Vec<u8>> {
        let decoded = self.current_decoder.decode(iter::once(packet));

        if decoded.is_some() {
            self.current_block_is_decoded = true;
            return decoded;
        }

        None
    }

    pub fn decode_stream(
        mut self,
        input: impl IntoIterator<Item = EncodingPacket>,
        output: &mpsc::SyncSender<Vec<u8>>,
    ) -> Result<(), BlockDecoderError> {
        for packet in input {
            if let Some(decoded_block) = self.decode(packet) {
                output.send(decoded_block)?;
            }
        }

        Ok(())
    }
}

// Infers the size of the packets that will be processed by the block decoder by encoding
// a dummy block.
pub fn infer_packet_size(
    block_size: usize,
    max_packet_size: u16,
) -> Result<usize, BlockDecoderError> {
    let mut encoder = match BlockEncoder::new(block_size, max_packet_size, 1) {
        Ok(e) => e,
        Err(e) => return Err(BlockDecoderError::InvalidConfiguration(e.to_string())),
    };

    let dummy_block = Block {
        id: 1,
        data: vec![0u8; block_size],
    };
    let mut packets = match encoder.encode(&dummy_block) {
        Ok(v) => v,
        Err(e) => return Err(BlockDecoderError::InvalidConfiguration(e.to_string())),
    };

    let packet = match packets.next() {
        Some(p) => p,
        None => {
            return Err(BlockDecoderError::InvalidConfiguration(
                "No packet was generated".to_string(),
            ));
        }
    };

    Ok(packet.serialize().len())
}

#[derive(Debug)]
pub enum BlockDecoderError {
    InvalidConfiguration(String),
    SendError(mpsc::SendError<Vec<u8>>),
}

impl fmt::Display for BlockDecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockDecoderError::InvalidConfiguration(s) => {
                write!(f, "{s}")
            }
            BlockDecoderError::SendError(e) => {
                write!(f, "{e}")
            }
        }
    }
}

impl From<ConfigurationError> for BlockDecoderError {
    fn from(err: ConfigurationError) -> BlockDecoderError {
        BlockDecoderError::InvalidConfiguration(err.to_string())
    }
}

impl From<mpsc::SendError<Vec<u8>>> for BlockDecoderError {
    fn from(err: mpsc::SendError<Vec<u8>>) -> BlockDecoderError {
        BlockDecoderError::SendError(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_encoder::BlockEncoder;
    use crate::random::get_random_vec;
    use std::thread;

    const BLOCK_SIZE: usize = 1496 * 192; // 280.5 KiB
    const MAX_PACKET_SIZE: u16 = 1500;
    const NUM_REPAIR_PACKETS: u32 = 15;

    #[test]
    fn test_encode_decode() {
        let source_block = get_random_vec(BLOCK_SIZE).unwrap();

        let mut encoder =
            BlockEncoder::new(source_block.len(), MAX_PACKET_SIZE, NUM_REPAIR_PACKETS).unwrap();

        let mut decoder = BlockDecoder::new(source_block.len(), MAX_PACKET_SIZE).unwrap();

        let mut packets = encoder
            .encode(&Block {
                id: 1,
                data: source_block.clone(),
            })
            .unwrap()
            .collect::<Vec<_>>();

        let mut decoded_block: Vec<u8> = Vec::new();
        for packet in packets.drain(..packets.len() - NUM_REPAIR_PACKETS as usize) {
            match decoder.decode(packet) {
                Some(v) => {
                    decoded_block = v;
                    break;
                }
                None => {}
            }
        }

        assert_eq!(decoded_block, source_block);

        // If the current block is already decoded, the next packet of the block should be ignored.
        assert!(decoder.decode(packets.remove(0)).is_none());
    }

    #[test]
    fn test_encode_decode_stream() {
        const NUM_BLOCKS: usize = 10;

        let source_blocks: Vec<Vec<u8>> = (0..NUM_BLOCKS)
            .into_iter()
            .map(|_| get_random_vec(BLOCK_SIZE).unwrap())
            .collect();

        let (input_tx, input_rx) = mpsc::sync_channel::<Block>(10);
        let (forward_tx, forward_rx) = mpsc::sync_channel::<EncodingPacket>(10);
        let (output_tx, output_rx) = mpsc::sync_channel::<Vec<u8>>(10);

        for (i, source_block) in source_blocks.iter().enumerate() {
            input_tx
                .send(Block {
                    id: i as u8,
                    data: source_block.clone(),
                })
                .unwrap();
        }
        drop(input_tx);

        let mut encoder =
            BlockEncoder::new(BLOCK_SIZE, MAX_PACKET_SIZE, NUM_REPAIR_PACKETS).unwrap();
        let decoder = BlockDecoder::new(BLOCK_SIZE, MAX_PACKET_SIZE).unwrap();

        let encoder_thread = thread::spawn(move || {
            encoder
                .encode_stream(input_rx.into_iter(), &forward_tx)
                .unwrap();
        });

        let decoder_thread = thread::spawn(move || {
            decoder
                .decode_stream(forward_rx.into_iter(), &output_tx)
                .unwrap();
        });

        encoder_thread.join().unwrap();
        decoder_thread.join().unwrap();

        let decoded_blocks = output_rx.into_iter().collect::<Vec<_>>();

        assert_eq!(decoded_blocks.len(), NUM_BLOCKS);
        assert_eq!(decoded_blocks[0].len(), BLOCK_SIZE);
        assert_eq!(decoded_blocks, source_blocks);
    }
}
