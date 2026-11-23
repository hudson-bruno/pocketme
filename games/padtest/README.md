# padtest: PADTEST

A game that shows everything a game receives, so each part of the input
can be seen to work:

- the 12 buttons (lit while held), both sticks (a dot in a box), both
  triggers (a bar);
- the touch: a red cross where the finger is, and its position;
- the motion sensor: a spirit level (the bubble goes to the high side)
  and the six numbers, or `NO MOTION` when the pad sends none.

It also tries the "only when something happens" frame rate. After one
second with no change it asks the console for it (the top line then
says `ON DEMAND`, and the counter of pictures drawn stops). At the first
change it asks for 30 frames a second again. A quarter-second timer
looks at the motion sensor while it sleeps, because motion alone does
not wake a game.

Uses only `game-kit`. No heap, no floating point. In the simulator: pick
the PADTEST tile. For the chip: `games/build-chip.sh`. Tests:
`cargo test` here (small parts) and `boards/desktop/tests/games.rs` (the
whole game in the console).
