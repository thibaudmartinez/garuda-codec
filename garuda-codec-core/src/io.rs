use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Reads the exact number of bytes required to fill buf.
///
/// When an interrupt or a timeout occurs, this function will check the value of the `stop_reading`
/// flag to decide whether it should keep filling the buffer. If `stop_reading` is set to true,
/// the whole read operation is aborted and `io::ErrorKind::Interrupted` is returned.
///
/// Based on the `read_exact` method of the `std::io::Read` trait.
pub fn read_exact(
    mut reader: impl io::Read,
    mut buf: &mut [u8],
    stop_reading: Arc<AtomicBool>,
) -> io::Result<()> {
    while !buf.is_empty() {
        match reader.read(buf) {
            Ok(0) => break,
            Ok(n) => {
                buf = &mut buf[n..];
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                ) =>
            {
                if stop_reading.load(Ordering::SeqCst) {
                    return Err(io::ErrorKind::Interrupted.into());
                }
            }
            Err(e) => return Err(e),
        }
    }

    if buf.is_empty() {
        Ok(())
    } else {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
}
