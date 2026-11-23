//! [`Absent`]: a stand-in for a part a board does not have.
//!
//! It takes no memory (it has no fields) and does nothing: no touch, no
//! buttons, no pad, no card, nothing heard. A board without GPS writes
//! `type Gps = Absent;` and keeps an `Absent` in its struct.

#[cfg(feature = "passby")]
use crate::optional::PASSBY_LEN;
use crate::{
    files::{DirEntry, Files, FsError},
    input::{Keys, Pad, PadState, Touch, TouchPoint},
    link::{Link, LogOut, Store, StoreError},
};

/// A part that is not there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Absent;

impl Touch for Absent {
    fn read(&mut self) -> Option<TouchPoint> {
        None
    }
}

impl Keys for Absent {
    fn held(&mut self) -> u16 {
        0
    }
}

impl Pad for Absent {
    fn read(&mut self, _index: u8) -> PadState {
        PadState::NONE
    }
}

impl Files for Absent {
    fn list<F: FnMut(&DirEntry<'_>) -> bool>(
        &mut self,
        _dir: &str,
        _each: F,
    ) -> Result<(), FsError> {
        Err(FsError::NoCard)
    }
    fn size(&mut self, _path: &str) -> Result<u32, FsError> {
        Err(FsError::NoCard)
    }
    fn read_at(&mut self, _path: &str, _offset: u32, _buf: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NoCard)
    }
}

impl Link for Absent {
    fn read(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
    /// Takes everything and drops it, so a sender never waits on it.
    fn write(&mut self, data: &[u8]) -> usize {
        data.len()
    }
}

impl LogOut for Absent {
    fn line(&mut self, _text: &str) {}
}

impl Store for Absent {
    fn size(&self) -> u32 {
        0
    }
    fn read(&mut self, _offset: u32, _buf: &mut [u8]) -> Result<(), StoreError> {
        Err(StoreError::OutOfRange)
    }
    fn write(&mut self, _offset: u32, _data: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::OutOfRange)
    }
    fn erase_sector(&mut self, _offset: u32) -> Result<(), StoreError> {
        Err(StoreError::OutOfRange)
    }
}

#[cfg(feature = "bt-classic")]
impl crate::optional::Hci for Absent {
    fn send(&mut self, _packet: &[u8]) {}
    fn recv(&mut self, _buf: &mut [u8]) -> Option<usize> {
        None
    }
}

#[cfg(feature = "passby")]
impl crate::optional::Beacon for Absent {
    fn advertise(&mut self, _msg: &[u8; PASSBY_LEN]) {}
    fn heard<F: FnMut(&[u8; PASSBY_LEN], i8)>(&mut self, _each: F) {}
}

#[cfg(feature = "gps")]
impl crate::optional::Gps for Absent {
    fn read(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
}

#[cfg(feature = "sound")]
impl crate::optional::Audio for Absent {
    /// Takes everything and drops it.
    fn push(&mut self, samples: &[i16]) -> usize {
        samples.len()
    }
}
