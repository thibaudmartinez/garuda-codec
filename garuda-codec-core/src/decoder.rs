use crate::block_decoder::{BlockDecoder, BlockDecoderError, infer_packet_size};
use crate::io::read_exact;
use raptorq::EncodingPacket;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::{fmt, io};
use std::{panic, thread};

pub struct Decoder {
    block_size: usize,
    num_block_decoders: usize,
    max_packet_size: u16,

    packet_dispatcher: Option<thread::JoinHandle<()>>,
    packet_dispatcher_should_stop: Arc<AtomicBool>,
    block_decoders: Vec<thread::JoinHandle<()>>,
    block_forwarder: Option<thread::JoinHandle<()>>,
}

impl Decoder {
    pub fn new(block_size: usize, num_block_decoders: usize, max_packet_size: u16) -> Self {
        Self {
            block_size,
            num_block_decoders,
            max_packet_size,
            packet_dispatcher: None,
            packet_dispatcher_should_stop: Arc::new(AtomicBool::new(false)),
            block_decoders: Vec::with_capacity(num_block_decoders),
            block_forwarder: None,
        }
    }

    pub fn start_decoding(
        &mut self,
        input_stream: impl io::Read + Send + 'static,
        mut output_stream: impl io::Write + Send + 'static,
    ) -> Result<(), DecoderError> {
        let (block_queue_tx, block_queue_rx) = mpsc::sync_channel(10);
        let mut packet_queues = Vec::with_capacity(self.num_block_decoders);

        // Start the block forwarder thread.
        self.block_forwarder = thread::spawn(move || {
            if let Err(e) = forward_blocks(&block_queue_rx, &mut output_stream) {
                eprintln!("Block forwarder error: {e:?}");
            }

            drop(output_stream);
        })
        .into();

        // Start the block decoder threads.
        for _ in 0..self.num_block_decoders {
            let (packet_queue_tx, packet_queue_rx) = mpsc::sync_channel::<EncodingPacket>(10);
            let block_queue_tx = block_queue_tx.clone();

            let block_decoder = BlockDecoder::new(self.block_size, self.max_packet_size)?;

            let handle = thread::spawn(move || {
                if let Err(e) = block_decoder.decode_stream(&packet_queue_rx, &block_queue_tx) {
                    eprintln!("Block decoder error: {e:?}");
                }

                drop(block_queue_tx);
            });

            self.block_decoders.push(handle);
            packet_queues.push(packet_queue_tx);
        }

        // Start the packet dispatcher thread.
        let mut packet_dispatcher =
            PacketDispatcher::new(infer_packet_size(self.block_size, self.max_packet_size)?);
        self.packet_dispatcher_should_stop = packet_dispatcher.should_stop.clone();

        self.packet_dispatcher = thread::spawn(move || {
            if let Err(e) = packet_dispatcher.dispatch(input_stream, &packet_queues) {
                eprintln!("Packet dispatcher error: {e:?}");
            }
        })
        .into();

        Ok(())
    }

    pub fn stop(&mut self) {
        self.packet_dispatcher_should_stop
            .store(true, Ordering::SeqCst);
    }

    pub fn wait(&mut self) -> Result<(), DecoderError> {
        let packet_dispatcher = self
            .packet_dispatcher
            .take()
            .ok_or(DecoderError::InvalidState(
                "Packet dispatcher not initialized".into(),
            ))?;

        let block_forwarder = self
            .block_forwarder
            .take()
            .ok_or(DecoderError::InvalidState(
                "Block forwarder not initialized".into(),
            ))?;

        // Wait for the packet dispatcher to stop.
        if let Err(e) = packet_dispatcher.join() {
            panic::resume_unwind(e)
        }

        // Wait for the block decoders to stop.
        for handle in self.block_decoders.drain(..) {
            if let Err(e) = handle.join() {
                panic::resume_unwind(e)
            }
        }

        // Wait for the block forwarder to stop.
        if let Err(e) = block_forwarder.join() {
            panic::resume_unwind(e)
        }

        Ok(())
    }
}

struct PacketDispatcher {
    packet_size: usize,
    should_stop: Arc<AtomicBool>,
}

impl PacketDispatcher {
    fn new(packet_size: usize) -> Self {
        Self {
            packet_size,
            should_stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn dispatch(
        &mut self,
        mut input_stream: impl io::Read,
        packet_queues: &[mpsc::SyncSender<EncodingPacket>],
    ) -> Result<(), PacketDispatcherError> {
        let mut packet = vec![0u8; self.packet_size];

        loop {
            match read_exact(&mut input_stream, &mut packet, self.should_stop.clone()) {
                Ok(()) => {
                    let packet = EncodingPacket::deserialize(&packet);
                    let received_block_id = packet.payload_id().source_block_number() as usize;

                    let packet_queue = &packet_queues[received_block_id % packet_queues.len()];
                    packet_queue.send(packet)?;
                }
                Err(ref e)
                    if matches!(e.kind(), ErrorKind::UnexpectedEof | ErrorKind::Interrupted) =>
                {
                    return Ok(());
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }
}

fn forward_blocks(
    block_queue: impl IntoIterator<Item = Vec<u8>>,
    mut output_stream: impl io::Write,
) -> io::Result<()> {
    for block in block_queue {
        output_stream.write_all(&block)?;
    }

    Ok(())
}

#[derive(Debug)]
pub enum DecoderError {
    BlockDecoder(BlockDecoderError),
    InvalidState(String),
}

impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockDecoder(e) => write!(f, "{e}"),
            Self::InvalidState(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for DecoderError {}

impl From<BlockDecoderError> for DecoderError {
    fn from(err: BlockDecoderError) -> Self {
        Self::BlockDecoder(err)
    }
}

#[derive(Debug)]
pub enum PacketDispatcherError {
    Send(mpsc::SendError<EncodingPacket>),
    IO(io::Error),
}

impl fmt::Display for PacketDispatcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send(e) => write!(f, "{e}"),
            Self::IO(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PacketDispatcherError {}

impl From<io::Error> for PacketDispatcherError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

impl From<mpsc::SendError<EncodingPacket>> for PacketDispatcherError {
    fn from(err: mpsc::SendError<EncodingPacket>) -> Self {
        Self::Send(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::Encoder;
    use crate::random::get_random_vec;
    use std::io::Read;

    const BLOCK_SIZE: usize = 1496 * 192; // 280.5 KiB
    const MAX_PACKET_SIZE: u16 = 1500;
    const NUM_REPAIR_PACKETS: u32 = 15;

    #[test]
    fn test_encode_decode() {
        const NUM_BLOCKS: usize = 10;

        let mut source_blocks: Vec<Vec<u8>> = (0..NUM_BLOCKS)
            .into_iter()
            .map(|_| get_random_vec(BLOCK_SIZE).unwrap())
            .collect();

        let input_stream: io::Cursor<Vec<u8>> =
            io::Cursor::new(source_blocks.clone().into_iter().flatten().collect());

        let (forward_stream_reader, forward_stream_writer) = io::pipe().unwrap();
        let (mut output_stream_reader, output_stream_writer) = io::pipe().unwrap();

        let mut encoder = Encoder::new(BLOCK_SIZE, MAX_PACKET_SIZE, NUM_REPAIR_PACKETS, 3);
        let mut decoder = Decoder::new(BLOCK_SIZE, 3, MAX_PACKET_SIZE);

        encoder
            .start_encoding(input_stream, forward_stream_writer)
            .unwrap();
        decoder
            .start_decoding(forward_stream_reader, output_stream_writer)
            .unwrap();

        let mut output_bytes = Vec::new();
        output_stream_reader.read_to_end(&mut output_bytes).unwrap();

        encoder.wait().unwrap();
        decoder.wait().unwrap();

        let mut decoded_blocks = output_bytes.chunks(BLOCK_SIZE).collect::<Vec<_>>();

        assert_eq!(decoded_blocks.len(), source_blocks.len());
        assert_eq!(decoded_blocks[0].len(), BLOCK_SIZE);

        decoded_blocks.sort();
        source_blocks.sort();
        assert_eq!(decoded_blocks, source_blocks);
    }
}
