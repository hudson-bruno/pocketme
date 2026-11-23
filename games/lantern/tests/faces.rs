//! Walls read the right way round, and doors slide the same way seen
//! from either side.
//!
//! The wall test reads the picture back: for each screen column it finds
//! which texture column was drawn (by matching every texel down the
//! column), and checks that the texture runs left to right on the screen
//! whichever way the player looks. Before the fix every face was drawn
//! mirrored (a door's handle on the wrong side).

use game_kit::Frame;
use lantern::art::{MATS, T_ROCK, TEX, TEXS};
use lantern::fixed::{ONE, centre};
use lantern::level::MAX_SIDE;
use lantern::render::{Detail, Renderer};
use lantern::world::{World, cell};

/// A closed room of `w` x `h` cells: rock all round, floor inside.
fn room(w: i32, h: i32) -> Box<World> {
    let mut world = Box::new(World::new(0));
    world.w = w;
    world.h = h;
    world.map = [cell::ROCK; 1024];
    world.open = [0; 1024];
    world.n_things = 0;
    world.glow = 0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            world.map[y as usize * MAX_SIDE + x as usize] = cell::FLOOR;
        }
    }
    world
}

/// Draws the NORMAL view (160 rays, one pixel each, from the top left).
fn draw(world: &World) -> (Box<Renderer>, Box<Frame>) {
    let mut r = Box::new(Renderer::new());
    let mut f = Box::new(Frame::new());
    r.draw(&mut f, world, Detail::Normal);
    (r, f)
}

/// The texture column drawn at screen column `x`, found by matching the
/// drawn texels against every column of the rock texture.
fn drawn_column(r: &Renderer, f: &Frame, x: usize) -> usize {
    let v = Detail::Normal.view();
    let focal = v.focal();
    let cy = v.y0 + v.h / 2;
    let perp = r.zbuf[x];
    let line = (focal * ONE) / perp;
    let t0 = cy - line / 2;
    let top = t0.max(v.y0);
    let bottom = (cy + line / 2).min(v.y0 + v.h);
    let step = ((TEX as i32) << 16) / line.max(1);
    let tex = &TEXS[T_ROCK];
    let mut best = (0, 0);
    for u in 0..TEX {
        let mut p = ((top - t0) * step) as u32;
        let mut hits = 0;
        for row in top..bottom {
            let drawn = f.0[row as usize * game_kit::W + x] % MATS as u8;
            let texel = tex[u * TEX + ((p >> 16) as usize & (TEX - 1))];
            if drawn == texel {
                hits += 1;
            }
            p = p.wrapping_add(step as u32);
        }
        if hits > best.1 {
            best = (u, hits);
        }
    }
    assert_eq!(
        best.1,
        bottom - top,
        "column {x}: no texture column matches every drawn texel"
    );
    best.0
}

#[test]
fn walls_read_left_to_right_whichever_way_the_player_looks() {
    for (angle, name) in [
        (0u16, "east"),
        (16_384, "south"),
        (32_768, "west"),
        (49_152, "north"),
    ] {
        let mut world = room(9, 9);
        world.x = centre(4);
        world.y = centre(4);
        world.angle = angle;
        let (r, f) = draw(&world);
        let cols: Vec<usize> = (0..160).map(|x| drawn_column(&r, &f, x)).collect();
        // Next column: the same texel or one or two further on (wrapping
        // at a cell's edge), never backwards.
        let mut forward = 0;
        for pair in cols.windows(2) {
            let d = (pair[1] + TEX - pair[0]) % TEX;
            assert!(
                d <= 2,
                "looking {name}: texture went from {} to {}",
                pair[0],
                pair[1]
            );
            forward += usize::from(d > 0);
        }
        assert!(
            forward > 100,
            "looking {name}: only {forward} steps forward"
        );
    }
}

#[test]
fn a_half_open_door_shows_its_gap_on_the_same_map_side_from_both_sides() {
    // A door in the middle of a room, half open; looked at from the west
    // (facing east) and from the east (facing west), 2 cells away.
    let mut world = room(11, 9);
    let door = 4 * MAX_SIDE + 5;
    world.map[door] = cell::DOOR;
    world.open[door] = 128;
    let mut gap_on_left = [false; 2];
    for (k, (x, angle)) in [(centre(3), 0u16), (centre(7), 32_768)]
        .into_iter()
        .enumerate()
    {
        world.x = x;
        world.y = centre(4);
        world.angle = angle;
        let (r, _) = draw(&world);
        // The door face is 1.5 cells away; through the gap the far wall
        // is 3.5 or more. Middle half of the view: the door's own width.
        let far = |x: usize| r.zbuf[x] > 2 * ONE;
        let left = (50..78).filter(|&x| far(x)).count();
        let right = (82..110).filter(|&x| far(x)).count();
        assert!(left + right > 20, "view {k}: no gap seen");
        assert!(left == 0 || right == 0, "view {k}: gap on both sides");
        gap_on_left[k] = left > 0;
    }
    // From the west the map's north (small y) is on the left; from the
    // east it is on the right. So the gap must swap screen sides.
    assert_ne!(
        gap_on_left[0], gap_on_left[1],
        "the gap is on the same screen side from both sides: the door slides two ways"
    );
}
