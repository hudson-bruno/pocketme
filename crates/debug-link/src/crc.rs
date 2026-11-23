//! The three checksums of the protocol. Small bit-by-bit and
//! nibble-table versions: the firmware pays a few hundred bytes of flash
//! for them instead of kilobytes of tables.

/// CRC-8 (polynomial 0x07, start 0). Guards the five header bytes, so
/// that junk which happens to contain the start marker is thrown away
/// at once instead of making the reader wait for a made-up length.
pub fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// CRC-16/CCITT-FALSE (polynomial 0x1021, start 0xFFFF), continued from
/// `crc`. Guards a whole frame.
pub fn crc16_update(mut crc: u16, bytes: &[u8]) -> u16 {
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// Start value for [`crc16_update`].
pub const CRC16_INIT: u16 = 0xFFFF;

/// CRC-32 (the one of zip and PNG), fed piece by piece. Guards a whole
/// picture, across all its frames: it catches a frame that went missing
/// or arrived twice, which the per-frame check cannot.
#[derive(Clone, Copy, Debug)]
pub struct Crc32(u32);

/// Sixteen entries, one per half byte.
const CRC32_NIBBLES: [u32; 16] = {
    let mut table = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 4 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

impl Crc32 {
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for &byte in bytes {
            crc ^= u32::from(byte);
            crc = (crc >> 4) ^ CRC32_NIBBLES[(crc & 0xF) as usize];
            crc = (crc >> 4) ^ CRC32_NIBBLES[(crc & 0xF) as usize];
        }
        self.0 = crc;
    }

    pub const fn finish(self) -> u32 {
        !self.0
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The standard check values: each algorithm run over "123456789".
    #[test]
    fn check_values() {
        assert_eq!(crc8(b"123456789"), 0xF4);
        assert_eq!(crc16_update(CRC16_INIT, b"123456789"), 0x29B1);
        let mut crc = Crc32::new();
        crc.update(b"1234");
        crc.update(b"56789");
        assert_eq!(crc.finish(), 0xCBF4_3926);
    }
}
