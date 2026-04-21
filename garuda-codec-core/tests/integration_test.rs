use garuda_codec_core::compute_repair_packets_per_block;
#[cfg(target_os = "linux")]
use garuda_codec_core::{Decoder, Encoder, RawSocket, random::get_random_vec};
#[cfg(target_os = "linux")]
use std::io::{ErrorKind, Read};
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::{io, thread};

#[cfg(target_os = "linux")]
#[test]
fn test_encode_decode_raw_sockets() {
    const BLOCK_SIZE: usize = 1496 * 192; // 280.5 KiB
    const MAX_PACKET_SIZE: u16 = 1500;
    const REDUNDANCY_FACTOR: u32 = 30;
    const NUM_WORKERS: usize = 3;

    const NUM_BLOCKS: usize = 25;

    let repair_packets_per_block =
        compute_repair_packets_per_block(BLOCK_SIZE, MAX_PACKET_SIZE, REDUNDANCY_FACTOR).unwrap();

    let mut source_blocks: Vec<Vec<u8>> = (0..NUM_BLOCKS)
        .into_iter()
        .map(|_| get_random_vec(BLOCK_SIZE).unwrap())
        .collect();

    let input_stream: io::Cursor<Vec<u8>> =
        io::Cursor::new(source_blocks.clone().into_iter().flatten().collect());

    let forward_stream_writer = RawSocket::new("origdev").unwrap();
    let forward_stream_reader = RawSocket::new("destdev").unwrap().bind().unwrap();

    let (mut output_stream_reader, output_stream_writer) = io::pipe().unwrap();

    let mut encoder = Encoder::new(
        BLOCK_SIZE,
        MAX_PACKET_SIZE,
        repair_packets_per_block,
        NUM_WORKERS,
    );
    let mut decoder = Decoder::new(BLOCK_SIZE, NUM_WORKERS, MAX_PACKET_SIZE);

    encoder
        .start_encoding(input_stream, forward_stream_writer)
        .unwrap();
    decoder
        .start_decoding(forward_stream_reader, output_stream_writer)
        .unwrap();

    let (output_bytes_tx, output_bytes_rx) = mpsc::channel();

    thread::spawn(move || {
        loop {
            let mut buffer = vec![0u8; BLOCK_SIZE];

            match output_stream_reader.read_exact(&mut buffer) {
                Ok(()) => {
                    output_bytes_tx.send(buffer).unwrap();
                }
                Err(ref e) if e.kind() == ErrorKind::UnexpectedEof => {
                    return;
                }
                Err(e) => panic!("{:?}", e),
            }
        }
    });

    encoder.wait().unwrap();

    thread::sleep(Duration::from_secs(5));
    decoder.stop();
    decoder.wait().unwrap();

    let mut decoded_blocks = output_bytes_rx.iter().collect::<Vec<_>>();

    assert_eq!(decoded_blocks.len(), source_blocks.len());
    assert_eq!(decoded_blocks[0].len(), BLOCK_SIZE);

    decoded_blocks.sort();
    source_blocks.sort();
    assert_eq!(decoded_blocks, source_blocks);
}
