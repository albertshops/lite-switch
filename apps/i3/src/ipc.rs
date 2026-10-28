//! Bounded, synchronous i3 IPC. Each request uses a fresh connection.
use serde::de::DeserializeOwned;
use std::{
    env,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const MAGIC: &[u8; 6] = b"i3-ipc";
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
pub const COMMAND: u32 = 0;
pub const GET_OUTPUTS: u32 = 3;
pub const GET_TREE: u32 = 4;
pub const GET_VERSION: u32 = 7;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn socket_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("I3SOCK").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }
    let output = Command::new("i3")
        .arg("--get-socketpath")
        .output()
        .map_err(|e| format!("cannot locate i3 socket: {e}; run inside i3 or set I3SOCK"))?;
    let path = String::from_utf8(output.stdout)?;
    if !output.status.success() || path.trim().is_empty() {
        return Err("cannot locate i3 socket; run inside i3 or set I3SOCK".into());
    }
    Ok(PathBuf::from(path.trim()))
}

pub fn connect(path: &Path) -> Result<UnixStream> {
    let stream = UnixStream::connect(path)
        .map_err(|e| format!("cannot connect to i3 at {}: {e}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(stream)
}

pub fn request<T: DeserializeOwned>(path: &Path, kind: u32, payload: &str) -> Result<T> {
    let mut stream = connect(path)?;
    write_frame(&mut stream, kind, payload.as_bytes())?;
    let (reply_kind, body) = read_frame(&mut stream)?;
    if reply_kind != kind {
        return Err(format!("unexpected i3 response type {reply_kind}, expected {kind}").into());
    }
    Ok(serde_json::from_slice(&body)?)
}

pub(crate) fn write_frame(writer: &mut impl Write, kind: u32, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "i3 payload too large",
        ));
    }
    // i3 4.23 loses partial headers if its nonblocking read hits EAGAIN.
    // Submit the complete header/payload together, not as four tiny writes.
    let mut frame = Vec::with_capacity(14 + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&kind.to_le_bytes());
    frame.extend_from_slice(payload);
    writer.write_all(&frame)
}

pub(crate) fn read_frame(reader: &mut impl Read) -> io::Result<(u32, Vec<u8>)> {
    let mut header = [0; 14];
    reader.read_exact(&mut header)?;
    if &header[..6] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid i3 IPC magic",
        ));
    }
    let length = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
    if length > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "i3 response too large",
        ));
    }
    let kind = u32::from_le_bytes(header[10..14].try_into().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok((kind, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip_and_little_endian_header() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, GET_TREE, b"{}").unwrap();
        assert_eq!(&bytes[..14], b"i3-ipc\x02\0\0\0\x04\0\0\0");
        assert_eq!(
            read_frame(&mut Cursor::new(bytes)).unwrap(),
            (GET_TREE, b"{}".to_vec())
        );
    }

    #[test]
    fn submits_header_and_payload_together_for_nonblocking_i3_servers() {
        struct Writer {
            writes: Vec<Vec<u8>>,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.writes.push(bytes.to_vec());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = Writer { writes: Vec::new() };
        write_frame(&mut writer, COMMAND, b"[con_id=42] focus").unwrap();
        assert_eq!(writer.writes.len(), 1);
        assert_eq!(&writer.writes[0][..6], MAGIC);
        assert_eq!(&writer.writes[0][14..], b"[con_id=42] focus");
    }

    #[test]
    fn rejects_bad_magic_oversized_and_truncated_frames() {
        assert!(read_frame(&mut Cursor::new([0; 14])).is_err());
        let mut bytes = Vec::new();
        write_frame(&mut bytes, GET_TREE, b"{}").unwrap();
        bytes.pop();
        assert!(read_frame(&mut Cursor::new(bytes)).is_err());
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&((MAX_PAYLOAD + 1) as u32).to_le_bytes());
        bytes.extend_from_slice(&GET_TREE.to_le_bytes());
        assert!(read_frame(&mut Cursor::new(bytes)).is_err());
    }

    #[test]
    fn reads_fragmented_frames() {
        struct OneByte(Cursor<Vec<u8>>);
        impl Read for OneByte {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let len = buf.len().min(1);
                self.0.read(&mut buf[..len])
            }
        }
        let mut bytes = Vec::new();
        write_frame(&mut bytes, GET_TREE, b"{}").unwrap();
        assert_eq!(
            read_frame(&mut OneByte(Cursor::new(bytes))).unwrap().1,
            b"{}"
        );
    }
}
