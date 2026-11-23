# game-3d: the games' shared 3D library

This crate draws **flat-shaded 3D polygon models** into the console's
160 x 120 picture, with whole numbers only. It is a *game library*:
like `game-kit`, it is compiled into each game that uses it, never into
the console. A game that does not use it pays nothing. A game that
does gets only the parts it calls.

It follows the game rules: `no_std`, no heap, no floating point, no
`unsafe`. It depends only on `game-kit` (for the picture,
`game_kit::Frame`).

KESTREL, CAMBER, PROTO3D and DRIVE3D use it today (KESTREL's
and CAMBER's renderers moved here on 2026-11-19, with every picture of
both games unchanged). MERCATOR still has its own copy; see "Who uses
what" below.

## Words used here

- **model**: a small table of corners and faces. A face is three corner
  numbers and a *material* (the colour family).
- **camera**: where the eye is and which way it looks. x is right, y is
  up, z is forward.
- **near plane**: nothing closer to the eye than this is drawn; faces
  that reach behind it are cut.
- **clipping**: cutting a face where it leaves the view, in 3D.
- **back face**: a face turned away from the eye; it is not drawn.
- **painter's order**: farther things are drawn first, nearer ones over
  them. There is no depth buffer.
- **shading**: the colour of a face from its material, how much it
  faces the light, and (with fog) how far away it is.

## Units

| What | Unit |
|---|---|
| positions, sizes | 1/256 m (`M8` = one metre) |
| screen positions inside the library | 1/16 pixel (`SUB`) |
| matrices, sine, cosine, directions | 1/16384 (`ONE14`) |
| angles | 1/65536 of a turn (`deg(90)` is a quarter turn) |

## Draw a model, step by step

```rust
use game_3d::{Camera, Mat3, Object, Rect, Renderer, Shading, Standard, View, M8};

// 1. Working memory: up to 48 corners and 112 triangles per model.
//    About 2.4 KB: keep it in the game's state, never on the stack.
let mut r: Renderer<Standard, 48, 112> = Renderer::new();

// 2. Where on the picture, and the lens (pixels per metre at 1 m).
let view = View::new(Rect::new(0, 0, 160, 120), 96);

// 3. The eye: 3 m behind the middle of the world, looking along +z.
let cam = Camera { pos: [0, 0, -3 * M8], rot: Mat3::IDENTITY };

// 4. The shading rule: 4 materials x 4 brightness steps in the palette
//    (palette number = step * mats + material), light from above.
let light = Shading {
    mats: 4, shades: 4, light: [0, -16384, 0], glow: 0,
    fog_from: 0, fog_to: 0, fog_shades: 0, clear: 0, dark_side: 3,
};

// 5. One object: a model, its place, its turn and its size (256 = own size).
let rock = Object { model: &MODEL, pos: [0, 0, 0], rot: Mat3::IDENTITY, scale: 256, swap: (0, 0) };

// 6. Each picture: begin, then draw objects far to near.
r.begin();
r.draw(&mut frame, &view, &cam, &light, &rock, 100 * M8); // drawn up to 100 m away
```

Models are written as tables and finished while compiling:
`orient` turns every face so its front looks out of its convex part
(nobody winds a table by hand), `normals` works out each face's
direction for the light, and `radius_of` the ball round the model.
KESTREL's `src/models.rs` shows how.

For a floor or a track made of many polygons that share corners (each
corner turned once), use `Pt` and `draw_poly` instead of a `Renderer`;
`tests/mesh.rs` shows it, and CAMBER's `src/scene.rs` draws its whole
track this way.

When the view never changes (one picture area, one lens), its five
planes can be worked out while compiling, because
`View::clip_planes_at` is a `const fn`:
`const PLANES: Planes = VIEW.clip_planes_at(MySetup::NEAR);`. Hand them
to `Pt::new`, `draw_poly` and `Renderer::draw_in` (the same as `draw`,
in the planes given); the game then carries no code that works them
out. CAMBER does this.

## How a model reaches the picture

1. Objects whose ball is wholly outside the view, or farther than
   `far`, are skipped.
2. Each corner is turned, scaled, moved and taken into the camera's
   view (one matrix: camera turn times object turn).
3. Back faces are dropped.
4. A face that crosses the near plane, or reaches far outside the view,
   is cut in 3D (one plane after the other). A face only a little
   outside is left to the filler (the "guard band").
5. Perspective: x and y divided by the distance, to 1/16 pixel.
6. The object's triangles are sorted far to near.
7. Each triangle is filled row by row, edges followed exactly with the
   "top-left rule": two triangles that share an edge never paint a
   pixel twice and never leave a gap.

## Choices made while compiling

Nothing here is decided while the game runs. A game names its choices
once, in a type that implements `Setup`; the compiler builds only those.
`Standard` is KESTREL's and the one to start from. `Sums` must be
named; any constant left out keeps `Standard`'s value.

```rust
struct Track;
impl game_3d::Setup for Track {
    type Sums = game_3d::Checked;
    const NEAR: i32 = game_3d::M8 / 4;
    const LENS: game_3d::Lens = game_3d::Lens::Fast;
    const SEAM_PROOF: bool = true;
}
```

| Choice | Where | Options | Standard |
|---|---|---|---|
| list sizes | `Renderer<C, V, T>` | corners `V`, triangles `T` (at most 256) per model | KESTREL: 48, 112 |
| sums | `Setup::Sums` | `Checked` (a sum that runs over stops the game), `Wrapping` (it wraps; smaller code) | `Checked` |
| perspective | `Setup::LENS` | `Wide` (64-bit; slow division on the chip), `Fast` (32-bit when the point is small enough, else 64-bit; same pixels as `Wide`), `Narrow` (32-bit only; the game keeps the numbers small) | `Wide` |
| near plane | `Setup::NEAR` | any distance in 1/256 m | `M8 / 2` (half a metre) |
| back faces | `Setup::CULL` | `Corners` (from the turned corners), `Normal` (from the model's face directions) | `Corners` |
| sort depth | `Setup::DEPTH` | `Middle` (mean of three corners), `Farthest` | `Middle` |
| crack-free cuts | `Setup::SEAM_PROOF` | `true`: an edge shared by two faces is cut at the very same point from both sides | `false` |
| shading | the `Shade` value handed to `draw` | `Shading` (light, fog, glow), `Lit<false>` (no fog), `Steps` (CAMBER's), `Flat` (material only) | `Shading` |
| far distance | `far`, handed to `draw` | any distance | none |
| object sizes | `Setup::SCALE` | `true`: `Object::scale` is read (256 = own size); `false`: every model at its own size, nine products less per object | `true` |
| whole-ball shortcut | `Setup::INSIDE_SHORTCUT` | `true`: when an object's ball is wholly inside the view, its corners skip the plane tests; `false`: every corner is tested (less code, the same picture) | `true` |
| corners per cut polygon | `draw_poly::<C, P>`, `clip_polygon_with::<C, P>`, `Renderer<C, V, T, P>` | `P` | `MAX_POLY` = 8 |
| what is counted | `Renderer<C, V, T, P, K>` and the counter handed to `Pt::new` / `draw_poly` | any type that implements `Count`: `Stats` (KESTREL's eight counts), `()` (nothing), or the game's own (CAMBER's counts projected corners and filler rows too) | `Stats` |

### Counting

The renderer and `draw_poly` tell a counter (`Count`) what they did:
an object drawn, corners turned, a face looked at, a face in front, a
face cut, a corner projected, a triangle filled (with its pixels and,
when the counter's `ROWS` is `true`, the rows the filler visited), a
triangle dropped. Every method does nothing unless the counter writes
it, and the compiler builds no code for one that does nothing. With
`ROWS` the filler is `fill_tri_counted` (it gives back pixels and rows,
`Filled`); without, `fill_tri_with` (pixels only). Both paint the very
same pixels (tested).

### Smooth sine

`sin` and `cos` step between the table's 1,024 entries a turn
(KESTREL's). `sin_fine` and `cos_fine` go in a straight line between
them (CAMBER's: a slow camera turn would otherwise jerk by about half a
pixel), and `Mat3::rotation_fine` builds a turning from them. They are
`const fn`, so tables can use them while compiling. `Mat3::col` gives a
turning's right, up or forward direction.

### Checked or wrapping sums

The chip build of a game has overflow checks on, unless the game turns
them off in `games/build-chip.sh` (CAMBER is the only one that does;
see `games/README.md`). With `Checked` and checks on, a sum in the
library that runs over stops the game with a panic. The
panic names the line in game-3d that did the sum (a file such as
`render.rs` or `fill.rs`), not the game line that called the library.
On the chip only the line number reaches the console, not the file
name. With `Wrapping`, the sum
wraps round silently: the code is smaller and faster, but a mistake in
the ranges gives a wrong picture instead of a stop. `Wrapping` still
stops in the PC tests' debug builds (`debug_assert`), so the tests find
such a mistake. Which one the games should use is the owner's choice;
KESTREL and CAMBER use `Checked`. In CAMBER's chip build the checks
are off, so there its `Checked` sums wrap round like `Wrapping` ones
(and every other plain sum of CAMBER's does too); its PC tests still
have them on.

## Files

| File | What |
|---|---|
| `src/lib.rs` | the overview (`//!` docs), the units, the re-exports |
| `src/setup.rs` | `Setup`, `Standard`, `Lens`, `Cull`, `Depth` |
| `src/sums.rs` | `Sums`, `Checked`, `Wrapping` |
| `src/fixed.rs` | sine table, `sin`/`cos` and the smooth `sin_fine`/`cos_fine`, `Mat3`, `isqrt` |
| `src/model.rs` | `Model`, `Face`, `orient`, `normals`, `radius_of` |
| `src/view.rs` | `View` (lens, perspective), `Camera`, the clip planes |
| `src/clip.rs` | cutting a polygon at the planes |
| `src/fill.rs` | `Rect`, the triangle filler (also one that counts rows, `Filled`) and its slow reference |
| `src/render.rs` | `Renderer` (`draw`, `draw_in`), `Object`, `Stats` |
| `src/count.rs` | `Count`: what a game counts while drawing |
| `src/mesh.rs` | `Pt`, `draw_poly`: shared corners |
| `src/shade.rs` | `Shade`, `Lit`/`Shading`, `Steps`, `Flat` |

## Tests

From this folder (the toolchain is pinned to 1.99.0):

```sh
cargo fmt --check
cargo clippy --offline --locked --all-targets --all-features -- -D warnings
cargo test --offline --locked
```

`boards/desktop/check.sh` runs the same and also builds the crate
`no_std` for the ESP32 and a bare PC.

| File | What it checks |
|---|---|
| `tests/fill.rs` | the fast filler paints exactly the slow reference's pixels (checked and wrapping sums), odd triangles, the clip rectangle, shared edges with no gap and no double pixel, the counting filler paints the same and counts the rows |
| `tests/clip.rs` | cutting at the near plane and every side; seam-proof cuts meet from both sides; a 16-corner polygon is cut whole |
| `tests/render.rs` | nothing mirrored, back faces, objects half off the view, wrapping = checked pictures, every lens, both back-face rules, both sort depths, the near plane, the far distance, short lists, material swaps, the same pictures without the whole-ball shortcut, objects without sizes, a game's own counter counts what `Stats` counts, `draw_in` with planes made while compiling draws what `draw` draws |
| `tests/mesh.rs` | a floor of shared corners (CAMBER's way) has no cracks and no overlaps, a polygon wholly outside one plane is not drawn |
| `tests/maths.rs` | sine, turnings, the square root, checked and wrapping sums, `orient`, the shading rules, the smooth sine meets the table and goes straight between, smooth turnings, columns |

The games' own pictures are checked by their golden tests
(`games/<name>/tests/golden.rs`).

## Who uses what

| Game | Uses |
|---|---|
| KESTREL | `Standard`, `Renderer<Standard, 48, 112>`, `Shading` |
| CAMBER | its own `Camber` setup (`Checked`, `NEAR = M8 / 4`, `Lens::Fast`, `Cull::Normal`, `Depth::Farthest`, `SEAM_PROOF`, no `SCALE`, no `INSIDE_SHORTCUT`), `Pt` and `draw_poly::<Camber, 16>` for the track with planes made while compiling, `Renderer<Camber, 28, 44, 16, its own Stats>` with `draw_in` and `Steps` for the ships, `sin_fine`, `cos_fine`, `rotation_fine`, `isqrt`, `orient_by_middle` |
| MERCATOR | not yet (own copy). Would need: `Wrapping` sums; still missing here: lines (laser bolts), its free-flight helpers, a 128-triangle sort |
