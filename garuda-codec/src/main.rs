use clap::{Args, Parser, Subcommand};
use garuda_codec_core::{
    Decoder, Encoder, Listener, OutputStream, compute_repair_packets_per_block,
};
use garuda_file_transfer::{receive, send};
use std::error::Error;

/// Garuda codec CLI
#[derive(Parser, Debug)]
#[command(name = "garuda", version, about = "Garuda codec CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the encoder
    Encoder(EncoderArgs),

    /// Run the decoder
    Decoder(CodecArgs),

    /// Send a file to the encoder
    Send(SendArgs),

    /// Receive files from the decoder
    Receive(ReceiveArgs),
}

/// Arguments shared between encoder and decoder
#[derive(Args, Debug)]
#[allow(clippy::doc_markdown)]
struct CodecArgs {
    /// Input [-, tcp://<host>:<port>, unix://<path>, af_packet://<interface>]
    #[arg(short = 'i', long)]
    input: String,

    /// Output [-, tcp://<host>:<port>, unix://<path>, af_packet://<interface>]
    #[arg(short = 'o', long)]
    output: String,

    /// Block size (in bytes)
    #[arg(short = 'b', long, default_value_t = 287_232)]
    block_size: usize,

    /// Maximum packet size (in bytes)
    #[arg(short = 'p', long, default_value_t = 1500)]
    max_packet_size: u16,

    /// Number of worker threads
    #[arg(short = 'w', long, default_value_t = 1)]
    workers: usize,
}

/// Encoder-specific arguments
#[derive(Args, Debug)]
struct EncoderArgs {
    #[command(flatten)]
    codec_args: CodecArgs,

    /// Redundancy factor (percentage of repair packets to generate per block)
    #[arg(short = 'r', long, default_value_t = 30)]
    redundancy: u32,
}

#[derive(Args, Debug)]
struct SendArgs {
    /// Path of the file to send
    #[arg(short = 'f', long)]
    file_path: String,

    /// Address of the encoder [-, tcp://<host>:<port>, unix://<path>]
    #[arg(short = 'o', long)]
    address: String,

    /// Block size (in bytes)
    #[arg(short = 'b', long, default_value_t = 287_232)]
    block_size: usize,
}

#[derive(Args, Debug)]
struct ReceiveArgs {
    /// Path of the directory to hold received files
    #[arg(short = 'd', long)]
    directory: String,

    /// Address of the server exposed to the decoder [-, tcp://<host>:<port>, unix://<path>]
    #[arg(short = 'i', long)]
    address: String,

    /// Block size (in bytes)
    #[arg(short = 'b', long, default_value_t = 287_232)]
    block_size: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Encoder(args) => run_encoder(args)?,
        Commands::Decoder(args) => run_decoder(args)?,
        Commands::Send(args) => send(&args.file_path, &args.address, args.block_size)?,
        Commands::Receive(args) => receive(&args.directory, &args.address, args.block_size)?,
    }

    Ok(())
}

fn run_encoder(args: &EncoderArgs) -> Result<(), Box<dyn Error>> {
    let repair_packets_per_block = compute_repair_packets_per_block(
        args.codec_args.block_size,
        args.codec_args.max_packet_size,
        args.redundancy,
    )?;

    let listener = Listener::from_uri(&args.codec_args.input)?;
    let output_stream = OutputStream::from_uri(&args.codec_args.output)?;

    eprintln!("Listening on {}", args.codec_args.input);
    eprintln!("Sending encoded packets to {}", args.codec_args.output);

    for input_stream in listener.incoming() {
        let mut encoder = Encoder::new(
            args.codec_args.block_size,
            args.codec_args.max_packet_size,
            repair_packets_per_block,
            args.codec_args.workers,
        );

        encoder.start_encoding(input_stream?, output_stream.try_clone()?)?;
        encoder.wait()?;
    }

    Ok(())
}

fn run_decoder(args: &CodecArgs) -> Result<(), Box<dyn Error>> {
    let listener = Listener::from_uri(&args.input)?;
    let output_stream = OutputStream::from_uri(&args.output)?;

    eprintln!("Listening on {}", args.input);
    eprintln!("Sending decoded blocks to {}", args.output);

    for input_stream in listener.incoming() {
        let mut decoder = Decoder::new(args.block_size, args.workers, args.max_packet_size);

        decoder.start_decoding(input_stream?, output_stream.try_clone()?)?;
        decoder.wait()?;
    }

    Ok(())
}
