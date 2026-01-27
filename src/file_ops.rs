use crate::io;

pub trait FileOps {
    fn write(&self, buf: &[u8]) -> Result<usize, io::Error>;
}
