//! The picture as an embedded-graphics `DrawTarget` (Cargo feature
//! `embedded-graphics`; off by default, and the plain helpers never need
//! it).
//!
//! The colour type is `Gray8`, read as "the gray level is the palette
//! number". The test-card crate uses the same idea for its 160x120
//! card, so its pictures draw straight into a [`Frame`].

use core::convert::Infallible;

use embedded_graphics_core::Pixel;
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::{Gray8, GrayColor};
use embedded_graphics_core::primitives::Rectangle;

use crate::frame::{Frame, H, W};

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

/// Saturates an embedded-graphics coordinate into `i16` (anything that
/// far out is off the picture anyway).
fn to_i16(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

impl DrawTarget for Frame {
    type Color = Gray8;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            self.pixel(to_i16(p.x), to_i16(p.y), c.luma());
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let w = area.size.width.min(u32::from(u16::MAX)) as u16;
        let h = area.size.height.min(u32::from(u16::MAX)) as u16;
        self.rect(
            to_i16(area.top_left.x),
            to_i16(area.top_left.y),
            w,
            h,
            color.luma(),
        );
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        Frame::clear(self, color.luma());
        Ok(())
    }
}
