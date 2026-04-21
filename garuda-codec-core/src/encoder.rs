use crate::block_encoder::{Block, BlockEncoder, BlockEncoderError};
use crate::io::read_exact;
use crate::random::get_random_u8;
use raptorq::EncodingPacket;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::{fmt, io, panic, thread};

pub struct Encoder {
    block_size: usize,
    max_packet_size: u16,
    repair_packets_per_block: u32,
    num_block_encoders: usize,

    block_dispatcher: Option<thread::JoinHandle<()>>,
    block_dispatcher_should_stop: Arc<AtomicBool>,
    block_encoders: Vec<thread::JoinHandle<()>>,
    packet_forwarder: Option<thread::JoinHandle<()>>,
}

impl Encoder {
    pub fn new(
        block_size: usize,
        max_packet_size: u16,
        repair_packets_per_block: u32,
        num_block_encoders: usize,
    ) -> Self {
        Self {
            block_size,
            max_packet_size,
            repair_packets_per_block,
            num_block_encoders,
            block_dispatcher: None,
            block_dispatcher_should_stop: Arc::new(AtomicBool::new(false)),
            block_encoders: Vec::with_capacity(num_block_encoders),
            packet_forwarder: None,
        }
    }

    pub fn start_encoding(
        &mut self,
        input_stream: impl io::Read + Send + 'static,
        mut output_stream: impl io::Write + Send + 'static,
    ) -> Result<(), EncoderError> {
        let mut block_queues = Vec::with_capacity(self.num_block_encoders);
        let (packet_queue_tx, packet_queue_rx) = mpsc::sync_channel(10);

        // Start the packet forwarder thread.
        self.packet_forwarder = thread::spawn(move || {
            if let Err(e) = forward_packets(&packet_queue_rx, &mut output_stream) {
                eprintln!("Packet forwarder error: {e:?}");
            }

            drop(output_stream);
        })
        .into();

        // Start the block encoder threads.
        for _ in 0..self.num_block_encoders {
            let mut block_encoder = BlockEncoder::new(
                self.block_size,
                self.max_packet_size,
                self.repair_packets_per_block,
            )?;

            let (block_queue_tx, block_queue_rx) = mpsc::sync_channel(10);
            let packet_queue_tx = packet_queue_tx.clone();

            let handle = thread::spawn(move || {
                if let Err(e) = block_encoder.encode_stream(&block_queue_rx, &packet_queue_tx) {
                    eprintln!("Block encoder error: {e:?}");
                }

                drop(packet_queue_tx);
            });

            self.block_encoders.push(handle);
            block_queues.push(block_queue_tx);
        }

        // Start the block dispatcher thread.
        let mut block_dispatcher = BlockDispatcher::new(self.block_size)?;
        self.block_dispatcher_should_stop = block_dispatcher.should_stop.clone();

        self.block_dispatcher = thread::spawn(move || {
            if let Err(e) = block_dispatcher.dispatch(input_stream, &block_queues) {
                eprintln!("Block dispatcher error: {e:?}");
            }

            drop(block_queues);
        })
        .into();

        Ok(())
    }

    pub fn stop(&mut self) {
        self.block_dispatcher_should_stop
            .store(true, Ordering::SeqCst);
    }

    pub fn wait(&mut self) -> Result<(), EncoderError> {
        let block_dispatcher = self
            .block_dispatcher
            .take()
            .ok_or(EncoderError::InvalidState(
                "Block dispatcher not initialized".into(),
            ))?;

        let packer_forwarder = self
            .packet_forwarder
            .take()
            .ok_or(EncoderError::InvalidState(
                "Packet forwarder not initialized".into(),
            ))?;

        // Wait for the block dispatcher to stop.
        if let Err(e) = block_dispatcher.join() {
            panic::resume_unwind(e)
        }

        // Wait for the block encoders to stop.
        for handle in self.block_encoders.drain(..) {
            if let Err(e) = handle.join() {
                panic::resume_unwind(e)
            }
        }

        // Wait for the packet forwarder to stop.
        if let Err(e) = packer_forwarder.join() {
            panic::resume_unwind(e)
        }

        Ok(())
    }
}

struct BlockDispatcher {
    block_size: usize,
    current_block_id: u8,
    should_stop: Arc<AtomicBool>,
}

impl BlockDispatcher {
    fn new(block_size: usize) -> Result<Self, BlockDispatcherError> {
        Ok(Self {
            block_size,
            current_block_id: get_random_u8()?,
            should_stop: Arc::new(AtomicBool::new(false)),
        })
    }

    fn dispatch(
        &mut self,
        mut input_stream: impl io::Read,
        block_queues: &[mpsc::SyncSender<Block>],
    ) -> Result<(), BlockDispatcherError> {
        loop {
            let mut buffer = vec![0u8; self.block_size];

            match read_exact(&mut input_stream, &mut buffer, self.should_stop.clone()) {
                Ok(()) => {
                    let block_queue =
                        &block_queues[self.current_block_id as usize % block_queues.len()];

                    let block = Block {
                        id: self.current_block_id,
                        data: buffer,
                    };
                    block_queue.send(block)?;

                    self.current_block_id = self.current_block_id.wrapping_add(1);
                }
                Err(ref e)
                    if matches!(e.kind(), ErrorKind::UnexpectedEof | ErrorKind::Interrupted) =>
                {
                    return Ok(());
                }
                Err(e) => {
                    return Err(BlockDispatcherError::IO(e));
                }
            }
        }
    }
}

fn forward_packets(
    packet_queue: impl IntoIterator<Item = EncodingPacket>,
    mut output_stream: impl io::Write,
) -> io::Result<()> {
    for packet in packet_queue {
        output_stream.write_all(&packet.serialize())?;
    }

    Ok(())
}

#[derive(Debug)]
pub enum EncoderError {
    BlockEncoder(BlockEncoderError),
    BlockDispatcher(BlockDispatcherError),
    IO(io::Error),
    InvalidState(String),
}

impl fmt::Display for EncoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockEncoder(e) => write!(f, "{e}"),
            Self::BlockDispatcher(e) => write!(f, "{e}"),
            Self::IO(e) => write!(f, "{e}"),
            Self::InvalidState(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for EncoderError {}

impl From<BlockEncoderError> for EncoderError {
    fn from(err: BlockEncoderError) -> Self {
        Self::BlockEncoder(err)
    }
}

impl From<BlockDispatcherError> for EncoderError {
    fn from(err: BlockDispatcherError) -> Self {
        Self::BlockDispatcher(err)
    }
}

impl From<io::Error> for EncoderError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

#[derive(Debug)]
pub enum BlockDispatcherError {
    Send(mpsc::SendError<Block>),
    IO(io::Error),
}

impl fmt::Display for BlockDispatcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send(e) => write!(f, "{e}"),
            Self::IO(e) => write!(f, "{e}"),
        }
    }
}

impl From<mpsc::SendError<Block>> for BlockDispatcherError {
    fn from(err: mpsc::SendError<Block>) -> Self {
        Self::Send(err)
    }
}

impl From<io::Error> for BlockDispatcherError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}
