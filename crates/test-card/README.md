# test-card

The test pictures of the debug link, as code that draws into anything
`embedded-graphics` can draw on. The firmware (`experiments/screen-mirror`)
draws them on the board and `tools/boardctl testcard` draws them on the
PC, so a screenshot can be compared with what should be there, pixel by
pixel.

* `draw_panel_card`: 320x240 RGB565, for the panel.
* `draw_fb_card`: 160x120, one palette index per pixel, with `PALETTE`.

```sh
cd crates/test-card && cargo test      # 3 tests, on the PC
```
