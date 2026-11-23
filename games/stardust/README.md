# stardust: STARDUST

A small toy: a comet catches stars. Steer with the d-pad, the left stick,
or a finger on the screen (the comet flies towards it). Each star caught
adds one to the count at the top. Start ends the toy.

It is here to prove the game path, not to be a finished game. It uses
pixel art from small tables, the palette (the stars twinkle by changing
one palette colour), input, and time: the comet moves by the time that
passed, so its speed does not depend on the frame rate. It runs at the
console's default rate, 30 frames a second. The stars come from a fixed
random seed, so every run is the same.

Uses only `game-kit`. No heap, no floating point (positions are in 1/256
of a pixel). In the simulator: pick the STARDUST tile. For the chip:
`games/build-chip.sh`. Tests: `cargo test` here and
`boards/desktop/tests/games.rs`.
