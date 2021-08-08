use std::io::{Read, Result, Write};

use crate::parse_error;

const MAX_PKT_SIZE: usize = 65516;

pub trait ReadExt {
    fn pkt_bin_read<'b>(&mut self, out: &'b mut Vec<u8>) -> Result<Option<&'b [u8]>>;
    fn pkt_text_read<'b>(&mut self, out: &'b mut Vec<u8>) -> Result<Option<&'b str>>;
}

impl<R: Read> ReadExt for R {
    fn pkt_bin_read<'b>(&mut self, out: &'b mut Vec<u8>) -> Result<Option<&'b [u8]>> {
        let mut len_hex = [0; 4];
        self.read_exact(&mut len_hex)?;

        let mut len_bytes = [0; 2];
        hex::decode_to_slice(&len_hex, &mut len_bytes).map_err(|_| parse_error!("bad hex len"))?;

        let mut len = u16::from_be_bytes(len_bytes) as usize;
        if len == 0 {
            return Ok(None);
        }
        len -= 4;
        if len > MAX_PKT_SIZE {
            return Err(parse_error!("max packet size exceeded"));
        } else if len == 0 {
            return Err(parse_error!("packet size is zero"));
        }

        out.reserve(len.saturating_sub(out.len()));
        out.resize(len, 0);
        self.read_exact(&mut out[..len])?;

        Ok(Some(out))
    }
    fn pkt_text_read<'b>(&mut self, out: &'b mut Vec<u8>) -> Result<Option<&'b str>> {
        let s = if let Some(s) = self.pkt_bin_read(out)? {
            s
        } else {
            return Ok(None);
        };
        if !s.ends_with(b"\n") {
            return Err(parse_error!("string should end with \n"));
        }
        Ok(Some(
            std::str::from_utf8(&s[..s.len() - 1]).map_err(|_| parse_error!("bad utf-8"))?,
        ))
    }
}

pub trait WriteExt {
    fn pkt_bin_write(&mut self, data: &[u8]) -> Result<()>;
    fn pkt_text_write(&mut self, data: &str) -> Result<()>;
    fn pkt_end(&mut self) -> Result<()>;
}

impl<W: Write> WriteExt for W {
    fn pkt_bin_write(&mut self, data: &[u8]) -> Result<()> {
        for chunk in data.chunks((MAX_PKT_SIZE - 4) as usize) {
            let len_bytes = (chunk.len() as u16 + 4).to_be_bytes();
            let mut len_hex = [0; 4];
            hex::encode_to_slice(&len_bytes, &mut len_hex).unwrap();
            self.write_all(&len_hex)?;
            self.write_all(chunk)?;
        }
        Ok(())
    }
    fn pkt_text_write(&mut self, data: &str) -> Result<()> {
        let mut string = data.to_string();
        string.push('\n');
        self.pkt_bin_write(string.as_bytes())
    }
    fn pkt_end(&mut self) -> Result<()> {
        self.write_all(b"0000")?;
        self.flush()?;
        Ok(())
    }
}
