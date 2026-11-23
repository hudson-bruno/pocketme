//! The 256 colours.

/// Makes an RGB565 colour (5 bits red, 6 green, 5 blue) from 8-bit red,
/// green and blue.
pub const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

/// 256 RGB565 colours (512 bytes) and a version number (4 bytes) that
/// changes whenever any colour changes. The renderer sends the whole
/// screen again after a change, and a board that keeps the palette in
/// another form rebuilds it.
#[derive(Clone, PartialEq, Eq)]
pub struct Palette {
    colours: [u16; 256],
    version: u32,
}

impl Palette {
    /// All black.
    pub const fn new() -> Self {
        Palette {
            colours: [0; 256],
            version: 0,
        }
    }

    pub fn get(&self, i: u8) -> u16 {
        self.colours[usize::from(i)]
    }

    /// Sets one colour. Setting the same colour again changes nothing.
    pub fn set(&mut self, i: u8, rgb565: u16) {
        if self.colours[usize::from(i)] != rgb565 {
            self.colours[usize::from(i)] = rgb565;
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Sets all 256 colours.
    pub fn set_all(&mut self, colours: &[u16; 256]) {
        if &self.colours != colours {
            self.colours = *colours;
            self.version = self.version.wrapping_add(1);
        }
    }

    /// The 256 colours, for [`crate::Video::game_buffers`]. Whoever
    /// writes them calls [`Palette::touch`] afterwards.
    pub(crate) fn colours_mut(&mut self) -> &mut [u16; 256] {
        &mut self.colours
    }

    pub fn colours(&self) -> &[u16; 256] {
        &self.colours
    }

    /// Changes whenever any colour changes.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Marks the palette as changed, so the next present sends the whole
    /// screen. For colours written through a raw pointer (see
    /// [`crate::Video::game_buffers`]), which the version cannot see.
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}
