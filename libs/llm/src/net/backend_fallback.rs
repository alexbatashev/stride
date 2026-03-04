use super::stream::Event;
use std::io;
use std::os::fd::RawFd;
use std::thread;

pub(super) struct Backend;

impl Backend {
    pub(super) fn new() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no reactor backend for this target",
        ))
    }

    pub(super) fn set_interest(&mut self, _fd: RawFd, _read: bool, _write: bool) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn delete(&mut self, _fd: RawFd) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn wait(&mut self, _timeout_ms: i32, _out: &mut Vec<Event>) -> io::Result<()> {
        thread::sleep(std::time::Duration::from_millis(25));
        Ok(())
    }
}
