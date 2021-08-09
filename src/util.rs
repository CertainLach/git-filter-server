use crate::ext::{ReadExt, WriteExt, MAX_PKT_SIZE};
use std::io::{Read, Result, Write};

/// Writes to inner buffer, wrapping input with pkt format
/// Doesn't sends flush sequences (0000)
pub struct WritePkt<W: Write> {
    buffer: Vec<u8>,
    write: W,
    written: u64,
}
impl<W: Write> WritePkt<W> {
    pub fn new(write: W) -> Self {
        Self {
            buffer: Vec::new(),
            write,
            written: 0,
        }
    }
    #[allow(dead_code)]
    pub fn written(&self) -> u64 {
        self.written
    }
    fn flush_buf(&mut self) -> Result<()> {
        self.write.pkt_bin_write(&self.buffer)?;
        self.written = self.written.saturating_add(self.buffer.len() as u64);
        self.buffer.truncate(0);
        Ok(())
    }
}
impl<W: Write> Write for WritePkt<W> {
    fn write(&mut self, mut buf: &[u8]) -> Result<usize> {
        if buf.len() == 0 {
            return Ok(0);
        }
        let len = buf.len();
        while buf.len() > 0 {
            let to_write = (MAX_PKT_SIZE - self.buffer.len()).min(buf.len());
            self.buffer.reserve(to_write);
            self.buffer.write_all(&buf[..to_write]).unwrap();
            if self.buffer.len() == MAX_PKT_SIZE {
                self.flush_buf()?;
            }
            buf = &buf[to_write..];
        }
        Ok(len)
    }

    fn flush(&mut self) -> Result<()> {
        self.flush_buf()?;
        self.write.flush()
    }
}

impl<W: Write> Drop for WritePkt<W> {
    fn drop(&mut self) {
        if self.buffer.len() > 0 {
            panic!("WritePkt was not flushed before drop")
        }
    }
}

/// Reads data in pkt format until receiving flush (0000)
pub struct ReadPktUntilFlush<R> {
    read: R,
    read_bytes: u64,
    buffer: Vec<u8>,
    offset: usize,
    eof: bool,
}
impl<R> ReadPktUntilFlush<R> {
    pub fn new(read: R) -> Self {
        Self {
            read,
            read_bytes: 0,
            buffer: Vec::new(),
            offset: 0,
            eof: false,
        }
    }
    pub fn finished(&self) -> bool {
        self.eof
    }
    #[allow(dead_code)]
    pub fn read(&self) -> u64 {
        self.read_bytes
    }
}
impl<R: Read> Read for ReadPktUntilFlush<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.eof {
            return Ok(0);
        }
        if self.buffer[self.offset..].is_empty() {
            match self.read.pkt_bin_read(&mut self.buffer)? {
                Some(_) => {}
                None => {
                    // Got flush
                    self.eof = true;
                    return Ok(0);
                }
            }
            assert!(
                !self.buffer.is_empty(),
                "pkt_bin_read never returns empty buffer"
            );
            self.offset = 0;
        }
        let data = &self.buffer[self.offset..];
        let read_bytes = data.len().min(buf.len());
        buf[..read_bytes].copy_from_slice(&data[..read_bytes]);
        self.offset += read_bytes;
        self.read_bytes = self.read_bytes.saturating_add(read_bytes as u64);

        Ok(read_bytes)
    }
}
