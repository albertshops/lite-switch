//! i3 subscriptions integrated with GTK's main loop: no tree polling/thread.
use crate::ipc::{self, Result};
use serde_json::Value;
use std::{
    io::{self, Read},
    os::unix::net::UnixStream,
    path::Path,
};

const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

pub struct Event {
    pub kind: u32,
    pub data: Value,
}

#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
}

impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Event>> {
        self.bytes.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut consumed = 0;
        while self.bytes.len() - consumed >= 14 {
            let header = &self.bytes[consumed..consumed + 14];
            if &header[..6] != b"i3-ipc" {
                return Err("invalid i3 event magic".into());
            }
            let length = u32::from_le_bytes(header[6..10].try_into()?) as usize;
            if length > MAX_PAYLOAD {
                return Err("i3 event payload too large".into());
            }
            let kind = u32::from_le_bytes(header[10..14].try_into()?);
            if kind & (1 << 31) == 0 {
                return Err("expected an i3 event frame".into());
            }
            let end = consumed + 14 + length;
            if end > self.bytes.len() {
                break;
            }
            events.push(Event {
                kind: kind & !(1 << 31),
                data: serde_json::from_slice(&self.bytes[consumed + 14..end])?,
            });
            consumed = end;
        }
        self.bytes.drain(..consumed);
        Ok(events)
    }
}

pub fn subscribe(path: &Path) -> Result<UnixStream> {
    let mut stream = ipc::connect(path)?;
    ipc::write_frame(
        &mut stream,
        2,
        br#"["window","workspace","output","shutdown"]"#,
    )?;
    let (kind, reply) = ipc::read_frame(&mut stream)?;
    let reply: Value = serde_json::from_slice(&reply)?;
    if kind != 2 || reply["success"] != true {
        return Err("i3 rejected event subscription".into());
    }
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}

pub fn read_available(stream: &mut UnixStream, decoder: &mut Decoder) -> Result<Vec<Event>> {
    let mut result = Vec::new();
    let mut bytes = [0; 8192];
    // Bound work per main-loop iteration during an event flood.
    for _ in 0..16 {
        match stream.read(&mut bytes) {
            Ok(0) => return Err("i3 closed its event stream".into()),
            Ok(length) => result.extend(decoder.feed(&bytes[..length])?),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(kind: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        ipc::write_frame(&mut bytes, kind | (1 << 31), body).unwrap();
        bytes
    }

    #[test]
    fn fragmented_and_coalesced_events() {
        let mut bytes = frame(3, br#"{"change":"title"}"#);
        bytes.extend(frame(0, br#"{"change":"focus"}"#));
        let mut decoder = Decoder::default();
        assert!(decoder.feed(&bytes[..5]).unwrap().is_empty());
        let events = decoder.feed(&bytes[5..]).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, 3);
        assert_eq!(events[1].data["change"], "focus");
        assert!(decoder.feed(&[]).unwrap().is_empty());
    }

    #[test]
    fn rejects_non_events_invalid_json_and_oversized_payloads() {
        let mut bytes = Vec::new();
        ipc::write_frame(&mut bytes, 2, b"{}").unwrap();
        assert!(Decoder::default().feed(&bytes).is_err());
        assert!(Decoder::default().feed(&frame(3, b"invalid")).is_err());
        let mut bytes = b"i3-ipc".to_vec();
        bytes.extend_from_slice(&((MAX_PAYLOAD + 1) as u32).to_le_bytes());
        bytes.extend_from_slice(&(3_u32 | 1 << 31).to_le_bytes());
        assert!(Decoder::default().feed(&bytes).is_err());
    }
}
