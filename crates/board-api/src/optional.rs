//! Optional parts. Each one exists only when its Cargo feature is on.
//! Off, the trait is gone, and so is every line of code that uses it.

/// Raw classic Bluetooth packets (HCI: the standard way to talk to a
/// Bluetooth radio). The gamepad host (crates/pad-proto) runs on top.
/// Feature `bt-classic`.
#[cfg(feature = "bt-classic")]
pub trait Hci {
    /// Sends one packet (its first byte says the kind: command, data).
    fn send(&mut self, packet: &[u8]);
    /// Takes the next packet from the radio into `buf`, if one waits.
    /// Returns its length.
    fn recv(&mut self, buf: &mut [u8]) -> Option<usize>;
}

/// The size of one pass-by message (fits in one Bluetooth Low Energy
/// advertisement).
#[cfg(feature = "passby")]
pub const PASSBY_LEN: usize = 27;

/// Pass-by messages: consoles nearby swap a short message, like the
/// Nintendo 3DS's StreetPass. Feature `passby`.
#[cfg(feature = "passby")]
pub trait Beacon {
    /// Sends this message over and over until a new one is set.
    fn advertise(&mut self, msg: &[u8; PASSBY_LEN]);
    /// Calls `each` for every message heard since the last call, with its
    /// signal strength in dBm (about -30 near, -90 far).
    fn heard<F: FnMut(&[u8; PASSBY_LEN], i8)>(&mut self, each: F);
}

/// A GPS module's raw serial bytes (the text lines it sends). Reading
/// them into a position is the console's job. Feature `gps`.
#[cfg(feature = "gps")]
pub trait Gps {
    /// Copies bytes that have arrived into `buf`. Returns how many.
    fn read(&mut self, buf: &mut [u8]) -> usize;
}

/// The sample rate of [`Audio`], in samples a second.
#[cfg(feature = "sound")]
pub const AUDIO_RATE: u32 = 22_050;

/// Sound out: 16-bit samples, one channel. Feature `sound`.
#[cfg(feature = "sound")]
pub trait Audio {
    /// Queues samples. Returns how many were taken (fewer when the
    /// board's buffer is full).
    fn push(&mut self, samples: &[i16]) -> usize;
}
