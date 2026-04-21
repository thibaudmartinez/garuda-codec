use crate::string::truncate_utf8;
use std::str::Utf8Error;
use std::{iter, str};
use uuid::Uuid;

const FILE_NAME_LEN: usize = 256;
pub const DATAGRAM_HEADER_LEN: usize = 16 + 8 + 8 + 4 + FILE_NAME_LEN;

#[derive(Debug)]
pub struct DatagramHeader {
    pub file_id: Uuid,
    pub file_length: u64,
    pub chunk_offset: u64,
    pub payload_length: u32,
    pub file_name: String,
}

impl DatagramHeader {
    pub fn new(
        file_id: Uuid,
        file_name: &str,
        file_length: u64,
        chunk_offset: u64,
        payload_length: u32,
    ) -> DatagramHeader {
        DatagramHeader {
            file_id,
            file_name: file_name.to_string(),
            file_length,
            chunk_offset,
            payload_length,
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(DATAGRAM_HEADER_LEN);

        // file_id
        buf.extend_from_slice(self.file_id.as_bytes());

        // file_length
        buf.extend_from_slice(&self.file_length.to_be_bytes());

        // chunk_offset
        buf.extend_from_slice(&self.chunk_offset.to_be_bytes());

        // payload_length
        buf.extend_from_slice(&self.payload_length.to_be_bytes());

        // file_name (fixed-size, null-padded)
        let file_name: String = truncate_utf8(&self.file_name, FILE_NAME_LEN)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        let name_bytes = file_name.as_bytes();
        buf.extend_from_slice(name_bytes);

        let padding = FILE_NAME_LEN - name_bytes.len();
        buf.extend(iter::repeat_n(0u8, padding));

        buf
    }

    pub fn deserialize(buf: &[u8]) -> Result<DatagramHeader, DatagramHeaderDeserializationError> {
        if buf.len() < DATAGRAM_HEADER_LEN {
            return Err(DatagramHeaderDeserializationError::BufferTooSmall);
        }

        let mut offset = 0;

        // file_id
        let file_id = Uuid::from_slice(&buf[offset..offset + 16])?;
        offset += 16;

        // file_length
        let file_length = u64::from_be_bytes(
            buf[offset..offset + 8]
                .try_into()
                .map_err(|_| DatagramHeaderDeserializationError::BufferTooSmall)?,
        );
        offset += 8;

        // chunk_offset
        let chunk_offset = u64::from_be_bytes(
            buf[offset..offset + 8]
                .try_into()
                .map_err(|_| DatagramHeaderDeserializationError::BufferTooSmall)?,
        );
        offset += 8;

        // payload_length
        let payload_length = u32::from_be_bytes(
            buf[offset..offset + 4]
                .try_into()
                .map_err(|_| DatagramHeaderDeserializationError::BufferTooSmall)?,
        );
        offset += 4;

        // file_name (fixed-size, null-padded)
        let raw_name = &buf[offset..offset + FILE_NAME_LEN];
        let end = raw_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(FILE_NAME_LEN);

        let file_name = str::from_utf8(&raw_name[..end])?.to_string();

        Ok(DatagramHeader {
            file_id,
            file_length,
            chunk_offset,
            payload_length,
            file_name,
        })
    }
}

#[derive(Debug)]
pub enum DatagramHeaderDeserializationError {
    BufferTooSmall,
    InvalidUuid(uuid::Error),
    InvalidUtf8(Utf8Error),
}

impl std::fmt::Display for DatagramHeaderDeserializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooSmall => write!(f, "buffer too small"),
            Self::InvalidUuid(e) => write!(f, "invalid UUID: {e}"),
            Self::InvalidUtf8(e) => write!(f, "file name contains invalid UTF-8: {e}"),
        }
    }
}

impl From<uuid::Error> for DatagramHeaderDeserializationError {
    fn from(err: uuid::Error) -> Self {
        Self::InvalidUuid(err)
    }
}

impl From<Utf8Error> for DatagramHeaderDeserializationError {
    fn from(err: Utf8Error) -> Self {
        Self::InvalidUtf8(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_header(file_name: &str) -> DatagramHeader {
        DatagramHeader::new(
            Uuid::parse_str("12345678-90ab-cdef-1234-567890abcdef").unwrap(),
            file_name,
            100,
            10,
            1,
        )
    }

    #[test]
    fn test_serialize_deserialize() {
        let header = make_header("test.txt");
        let deserialized = DatagramHeader::deserialize(&header.serialize()).unwrap();

        assert_eq!(deserialized.file_id, header.file_id);
        assert_eq!(deserialized.file_length, header.file_length);
        assert_eq!(deserialized.chunk_offset, header.chunk_offset);
        assert_eq!(deserialized.payload_length, header.payload_length);
        assert_eq!(deserialized.file_name, header.file_name);
    }

    #[test]
    fn test_deserialize_buffer_too_small() {
        let buf = vec![0u8; DATAGRAM_HEADER_LEN - 1];
        let result = DatagramHeader::deserialize(&buf);

        assert!(matches!(
            result,
            Err(DatagramHeaderDeserializationError::BufferTooSmall)
        ));
    }

    #[test]
    fn test_file_name_truncation() {
        let long_name = "a".repeat(FILE_NAME_LEN * 2);

        let header = make_header(&long_name);
        let deserialized = DatagramHeader::deserialize(&header.serialize()).unwrap();

        assert!(deserialized.file_name.len() <= FILE_NAME_LEN);
        assert_eq!(deserialized.file_id, header.file_id);
    }
}
