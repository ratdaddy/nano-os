#[derive(Debug, Clone, Copy)]
pub enum Error {
    UnexpectedEof,
    InvalidUtf8,
    IoFailure,
    InvalidInput,
}

pub enum SeekFrom {
    Start(usize),
    Current(isize),
}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;

    fn read_to_string(&mut self, out: &mut alloc::string::String) -> Result<(), Error>
    {
        let mut buf = [0u8; 256];
        loop {
            let len = self.read(&mut buf)?;
            if len == 0 {
                break;
            }
            let s = core::str::from_utf8(&buf[..len]).map_err(|_| Error::InvalidUtf8)?;
            out.push_str(s);
        }
        Ok(())
    }

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), Error> {
        while !buf.is_empty() {
            let n = self.read(buf)?;
            if n == 0 {
                return Err(Error::UnexpectedEof);
            }
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

pub trait Seek {
    fn seek(&mut self, pos: SeekFrom) -> Result<(), Error>;
}
