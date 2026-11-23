//! The pass-by beacon: what one console tells any other within reach.
//!
//! One old-style ("legacy") advertisement holds 31 bytes. Four go to the
//! field's own header (length, type 0xFF "manufacturer data", and a 16-bit
//! company number), which leaves 27 for us. This chip's radio is Bluetooth
//! 4.2 and has no larger kind of advertisement.
//!
//! The 27 bytes, in the layout sketched in the project's research notes:
//!
//! | bytes  | what                                                        |
//! |--------|-------------------------------------------------------------|
//! | 0      | format version                                              |
//! | 1      | flags (none defined yet)                                    |
//! | 2..8   | rotating id: random, changed together with the address      |
//! | 8..10  | look seed: the character's appearance, as a small number    |
//! | 10..12 | nickname: two indexes into a built-in word list             |
//! | 12..14 | id of the game being played                                 |
//! | 14..22 | game card: 8 bytes the game chooses                         |
//! | 22..26 | echo: start of the rotating id of the last beacon we heard  |
//! | 26     | spare                                                       |
//!
//! Nothing typed by a person, no position, and no number that stays the
//! same from one start to the next.

/// Company number 0xFFFF is set aside by the Bluetooth SIG for tests. A
/// real product would need a number of its own or a 16-bit service UUID.
pub const COMPANY: u16 = 0xFFFF;

pub const VERSION: u8 = 1;

pub const PAYLOAD_LEN: usize = 27;

/// The whole advertisement: 4 bytes of header and the payload.
pub const ADV_LEN: usize = 4 + PAYLOAD_LEN;

/// What this console sends.
#[derive(Clone, Copy)]
pub struct Beacon {
    pub id: [u8; 6],
    pub look: u16,
    pub nickname: [u8; 2],
    pub game: u16,
    pub card: [u8; 8],
    pub echo: [u8; 4],
}

impl Beacon {
    /// The 31 bytes that go on air.
    pub fn advertisement(&self) -> [u8; ADV_LEN] {
        let mut adv = [0; ADV_LEN];
        // Field header: the length counts the type byte and all after it.
        adv[0] = (ADV_LEN - 1) as u8;
        adv[1] = 0xFF;
        adv[2..4].copy_from_slice(&COMPANY.to_le_bytes());
        let p = &mut adv[4..];
        p[0] = VERSION;
        p[1] = 0;
        p[2..8].copy_from_slice(&self.id);
        p[8..10].copy_from_slice(&self.look.to_le_bytes());
        p[10..12].copy_from_slice(&self.nickname);
        p[12..14].copy_from_slice(&self.game.to_le_bytes());
        p[14..22].copy_from_slice(&self.card);
        p[22..26].copy_from_slice(&self.echo);
        adv
    }
}

/// What was read out of another console's beacon. Every field is taken as
/// a plain number; a game must still check that each is within its range.
#[derive(Clone, Copy)]
pub struct Heard {
    pub id: [u8; 6],
    pub look: u16,
    pub nickname: [u8; 2],
    pub game: u16,
    pub echo: [u8; 4],
    pub rssi: i8,
}

impl Heard {
    pub fn from_payload(p: &[u8; PAYLOAD_LEN], rssi: i8) -> Self {
        Self {
            id: [p[2], p[3], p[4], p[5], p[6], p[7]],
            look: u16::from_le_bytes([p[8], p[9]]),
            nickname: [p[10], p[11]],
            game: u16::from_le_bytes([p[12], p[13]]),
            echo: [p[22], p[23], p[24], p[25]],
            rssi,
        }
    }
}
