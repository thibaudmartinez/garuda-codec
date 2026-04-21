use std::fs::File;
use std::io;
use std::io::Read;

/// Generates a vector of random bytes using the operating system's
/// entropy source.
pub fn get_random_vec(size: usize) -> Result<Vec<u8>, io::Error> {
    let mut data = vec![0u8; size];
    let mut file = File::open("/dev/urandom")?;
    file.read_exact(&mut data)?;
    Ok(data)
}

/// Returns a single random `u8` generated from the operating system's
/// entropy source.
pub fn get_random_u8() -> Result<u8, io::Error> {
    let data = get_random_vec(1)?;
    Ok(data[0])
}
