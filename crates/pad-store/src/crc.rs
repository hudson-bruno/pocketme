//! CRC-32, the common kind (the one in zip files and Ethernet).
//!
//! A check sum over a record: if even one bit of the record changed (a
//! write cut by a power loss, a worn-out flash cell), the sum no longer
//! matches and the record is ignored. Computed bit by bit: slow but tiny,
//! and a record is only 28 bytes.

/// The CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_more(0, data)
}

/// The CRC-32 of a piece read in parts: `crc32_more(crc32(a), b)` is the
/// CRC-32 of `a` then `b` (start with 0). The same sum as settings-store's
/// `crc32_more`, so a firmware can print one and compare with the other.
pub fn crc32_more(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_value() {
        // The standard check value for this CRC.
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(super::crc32(b""), 0);
    }

    #[test]
    fn in_parts() {
        let d = b"123456789";
        for i in 0..=d.len() {
            assert_eq!(super::crc32_more(super::crc32(&d[..i]), &d[i..]), 0xCBF4_3926);
        }
    }
}
