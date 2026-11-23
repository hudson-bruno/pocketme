//! Calling a game through `UpdateFn`/`ExitFn` and a `LiveFrame`: the
//! frame always points at the buffers it was made from, with their sizes.

use game_api::{
    API_VERSION, ExitFn, GameCallbacks, GameFrame, Input, LiveFrame, PICTURE_BYTES, UpdateFn,
};

#[repr(C, align(4))]
struct Picture([u8; PICTURE_BYTES + 4]);

/// A game that paints its whole picture and the first colour, and logs.
unsafe extern "C" fn paint(f: *mut GameFrame) -> u32 {
    // SAFETY: called through `UpdateFn::call`, so `f` is a live frame.
    unsafe {
        let f = &mut *f;
        let n = usize::from(f.stride) * usize::from(f.height);
        core::ptr::write_bytes(f.pixels, 9, n);
        f.palette.write(0xABCD);
        f.log.write(b'!');
        f.log_len = 1;
        f.wake_at_us = (*f.input).size.into();
    }
    0
}

unsafe extern "C" fn mark(f: *mut GameFrame, reason: u32) {
    // SAFETY: as above.
    unsafe { (*f).frame = reason }
}

#[test]
fn a_live_frame_points_at_its_buffers_with_their_sizes() {
    let mut pic = Box::new(Picture([0; PICTURE_BYTES + 4]));
    let mut palette = [0u16; 256];
    let input = Input::new();
    let mut log = [0u8; 40];
    let numbers = GameFrame {
        // Wrong on purpose: the frame takes these from the buffers.
        size: 1,
        version: 99,
        pixels: 8 as *mut u8,
        width: 320,
        height: 240,
        stride: 4000,
        log_cap: 9000,
        game_colours: 9000,
        // Kept as given.
        frame: 5,
        time_us: 77,
        ..GameFrame::new()
    };
    let pixels: &mut [u8; PICTURE_BYTES] = (&mut pic.0[..PICTURE_BYTES]).try_into().unwrap();
    let live = LiveFrame::new(numbers, pixels, &mut palette, &input, &mut log).unwrap();
    let f = *live.get();
    assert_eq!(f.size as usize, size_of::<GameFrame>());
    assert_eq!(f.version, API_VERSION);
    assert_eq!((f.width, f.height, f.stride), (160, 120, 160));
    assert_eq!(f.log_cap, 40);
    assert_eq!(f.game_colours, 256, "never more than the palette has");
    assert_eq!((f.frame, f.time_us), (5, 77));

    // SAFETY: `paint` and `mark` keep the contract.
    let update = unsafe { UpdateFn::new(paint) };
    let exit = unsafe { ExitFn::new(mark) };
    let cb = GameCallbacks {
        size: size_of::<GameCallbacks>() as u32,
        version: API_VERSION,
        update,
        exit,
    };
    let mut live = live;
    cb.update.call(&mut live);
    let after = *live.get();
    cb.exit.call(&mut live, 3);
    assert_eq!(live.get().frame, 3);
    // `live` is not used again, so the buffers can be read now.
    assert_eq!(after.log_len, 1);
    assert_eq!(after.wake_at_us, size_of::<Input>() as u64);
    assert!(pic.0[..PICTURE_BYTES].iter().all(|&p| p == 9));
    assert_eq!(pic.0[PICTURE_BYTES..], [0; 4], "nothing past the picture");
    assert_eq!(palette[0], 0xABCD);
    assert_eq!(log[0], b'!');
}

#[test]
fn a_picture_off_a_4_byte_boundary_is_refused() {
    let mut pic = Box::new(Picture([0; PICTURE_BYTES + 4]));
    let mut palette = [0u16; 256];
    let input = Input::new();
    let pixels: &mut [u8; PICTURE_BYTES] = (&mut pic.0[1..=PICTURE_BYTES]).try_into().unwrap();
    assert!(LiveFrame::new(GameFrame::new(), pixels, &mut palette, &input, &mut []).is_none());
}

#[test]
fn the_wrappers_are_just_the_function_pointer() {
    assert_eq!(size_of::<UpdateFn>(), size_of::<fn()>());
    assert_eq!(size_of::<ExitFn>(), size_of::<fn()>());
    assert_eq!(size_of::<Option<UpdateFn>>(), size_of::<fn()>());
}

/// A game reads `input.size` bytes of the input (at most its own
/// `Input`'s size). An input that says it is bigger than it is would
/// send a game built for a later version, with a bigger `Input`, past
/// its end. So a frame with such an input is refused, as an odd picture
/// is. (Second reader, step 3: before, `LiveFrame::new` took any input,
/// so safe code could point a later game past the input.)
#[test]
fn an_input_that_says_it_is_bigger_than_it_is_is_refused() {
    let mut pic = Box::new(Picture([0; PICTURE_BYTES + 4]));
    let mut palette = [0u16; 256];
    let mut log = [0u8; 8];
    let pixels: &mut [u8; PICTURE_BYTES] = (&mut pic.0[..PICTURE_BYTES]).try_into().unwrap();
    let real = size_of::<Input>() as u32;
    for (size, ok) in [
        (real + 4, false),
        (u32::MAX, false),
        (real, true),
        (8, true),
    ] {
        let input = Input {
            size,
            ..Input::new()
        };
        let live = LiveFrame::new(GameFrame::new(), pixels, &mut palette, &input, &mut log);
        assert_eq!(live.is_some(), ok, "input size {size}");
    }
}
