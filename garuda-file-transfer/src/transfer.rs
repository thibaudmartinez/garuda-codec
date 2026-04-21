use crate::protocol::{DATAGRAM_HEADER_LEN, DatagramHeader};
use garuda_codec_core::{InputStream, Listener, OutputStream};
use std::error::Error;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::{fs, io};

pub fn send(file_path: &str, output_uri: &str, block_size: usize) -> Result<(), Box<dyn Error>> {
    let mut stream = OutputStream::from_uri(output_uri)?;

    let file_id = uuid::Uuid::new_v4();
    let file_name = Path::new(file_path)
        .file_name()
        .ok_or(io::Error::other("invalid filename"))?
        .to_str()
        .ok_or(io::Error::other("cannot convert filename to string"))?;

    let file_length = fs::metadata(file_path)?.len();

    let mut file = File::open(file_path)?;

    let payload_size = block_size - DATAGRAM_HEADER_LEN;
    let mut buffer = vec![0u8; payload_size];

    let mut offset = 0u64;

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }

        let header =
            DatagramHeader::new(file_id, file_name, file_length, offset, u32::try_from(n)?);
        // Write the header.
        stream.write_all(&header.serialize())?;
        // Write the payload with potential padding.
        stream.write_all(&buffer)?;

        offset += n as u64;
    }

    Ok(())
}

pub fn receive(
    output_path: &str,
    input_uri: &str,
    block_size: usize,
) -> Result<(), Box<dyn Error>> {
    let output_path = Path::new(output_path);
    // Create the output directory if it doesn't exist.
    fs::create_dir_all(output_path)?;

    let listener = Listener::from_uri(input_uri)?;

    for stream in listener.incoming() {
        let stream = stream?;

        match process_stream(output_path, stream, block_size) {
            Ok(()) => (),
            Err(e) => eprintln!("Error processing stream: {e}"),
        }
    }

    Ok(())
}

fn process_stream(
    output_path: &Path,
    mut stream: InputStream,
    block_size: usize,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = vec![0u8; block_size];

    loop {
        stream.read_exact(&mut buffer)?;

        let header = match DatagramHeader::deserialize(&buffer) {
            Ok(d) => d,
            Err(_) => {
                continue;
            }
        };
        let payload =
            &buffer[DATAGRAM_HEADER_LEN..DATAGRAM_HEADER_LEN + header.payload_length as usize];

        let file_path = output_path.join(&header.file_name);

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&file_path)?;

        file.seek(SeekFrom::Start(header.chunk_offset))?;
        file.write_all(payload)?;
    }
}
