# tools/media: the documentation's pictures and clips

Two scripts, both pure Python standard library (no Pillow, no ffmpeg, no
ImageMagick), always run as `python3 -I`:

- `pocketmedia.py` turns the simulator's PNG pictures (`boards/desktop`,
  `--shot` / `--png`) into small files: sharp still pictures and short
  animated GIF clips.
- `make.py` builds everything in `docs/media/` and `docs/gallery.md`
  again from the scenes in `scenes/`.

## Building the documentation's media again: make.py

```bash
# from the repository root, with a built simulator (default features)
python3 -I tools/media/make.py --sim boards/desktop/target/debug/pocketme-sim
python3 -I tools/media/make.py --sim PATH/TO/pocketme-sim camber lander   # only these scenes
python3 -I tools/media/make.py --reuse                                    # raw frames already there
```

For each scene it runs the simulator with no window, keeps the frames the
scene names, and writes the stills as PNG and the clips as GIF, both at
twice the size (`--scale`, default 2), into `docs/media/`. A run of every
scene also writes `docs/gallery.md`: the clips only, two side by side,
each with its caption (the stills are for the other pages, such as the
table of games in the root README). The raw frames go to
one folder per scene in a git-ignored work folder (`--work`; never in the
repository). The simulator is deterministic, so the same scene gives
the same bytes each time; a file in `docs/media/` that changes after a run
means the console or a game draws something else now.

A scene is one entry of a `scenes/*.json` list and one input script
(`scenes/<name>.script`, in the simulator's script language, see
`boards/desktop/README.md`, "The input script"):

| Key | Meaning |
|---|---|
| `scene` | the scene's name (and its work folder) |
| `game` | which heading of the gallery it goes under (`console` for the console's own screens) |
| `script` | the input script's file name |
| `boot` | the simulator's `--boot`: `full`, `short` or `off` |
| `fps` | the simulator's `--fps`: 30 or 60 |
| `frames` | how many frames to run |
| `stills` | a list of `{frame, name, caption}`: `docs/media/<name>.png` |
| `clips` | a list of `{from, to, every, name, caption}` (each with an optional `hold`, the last frame's extra hundredths of a second, default 80): `docs/media/<name>.gif`. One run of the script can give several clips, each a different stretch of it. A single `clip` (one such entry, or `null`) is still read |

The scripts are played by a script, not a person: a clip shows what the
game draws, not how well it can be played. The pictures say nothing about
speed either (the simulator calls a game on time whatever its work costs).

## pocketmedia.py

## Commands

```bash
# a still picture: palette PNG when it has at most 256 colours, else RGB;
# zlib level 9; --scale N repeats every pixel N x N (stays sharp)
python3 -I pocketmedia.py still IN.png OUT.png [--scale N]

# a clip from the simulator's frame-NNNNN.png files, frames A to B included
python3 -I pocketmedia.py gif OUT.gif --dir FRAMES_DIR --from A --to B \
    [--every K] [--fps F] [--scale N] [--loop L] [--hold-last CS]

# the same from a list of files (each file counts as K frames)
python3 -I pocketmedia.py gif OUT.gif a.png b.png c.png [--every K] [--fps F]

# size, colours, and for a GIF the frame count, length and loop
python3 -I pocketmedia.py info FILE [FILE ...]
```

`gif` options:

| Option | Meaning |
|---|---|
| `--every K` | take every K-th frame (default 1) |
| `--fps F` | the simulator's frame rate, 30 or 60 (default 30); one GIF frame lasts K/F s |
| `--scale N` | whole-number scale, nearest neighbour |
| `--loop L` | 0 = for ever (default), N = N more plays, -1 = no loop extension (plays once) |
| `--hold-last CS` | add CS hundredths of a second to the last frame |

It prints one line: frames in, frames written, size in bytes, length in
seconds. Notes (a frame with too many colours, too short delays) go to
stderr.

Making frames, from `boards/desktop` (`SCRATCH` is any scratch folder
that git ignores, so the pictures never go into the repository):

```bash
SHOTS=$(python3 -I -c 'print(",".join(map(str, range(150, 300))))')
pocketme-sim --quiet --frames 300 --script play.txt --shot "$SHOTS" --out "$SCRATCH/clip"
python3 -I ../../tools/media/pocketmedia.py gif "$SCRATCH/clip.gif" --dir "$SCRATCH/clip" --from 150 --to 299
```

## What the GIF writer does

- One global colour table when the whole clip has at most 255 colours (one
  index stays free for "transparent"); otherwise a local table per frame,
  holding only the colours that frame needs.
- A frame with more than 256 colours is reduced (low bits dropped, then the
  least used colour merged into its nearest one, until 256 remain) and a
  note says so. Simulator pictures never need this: the console draws with
  a palette.
- Frames after the first hold only the box of changed pixels. Inside the
  box, unchanged pixels are written as "transparent" when that makes the
  frame smaller (both ways are tried). Disposal is 1 (leave in place).
- Frames with no change are merged into one longer frame.
- Delays are rounded to hundredths with the error carried forward, so the
  clip keeps its real length. Delays under 2/100 s are raised to 2/100 s
  with a note (browsers show such frames as 10/100 s); at 60 fps use
  `--every 2`.
- LZW with clear code, end code, variable code size up to 12 bits, table
  reset at 4096 codes, sub-blocks of at most 255 bytes.

Input PNGs may be 8-bit RGB, RGBA, palette or grey, not interlaced, any
filter. Other kinds are refused with a message. See-through RGBA pixels are
put on black (GIF has no partial transparency).

## Tests

```bash
cd tools/media && python3 -I -B -m unittest -v    # -B: no __pycache__ folder
```

The tests carry their own GIF reader (LZW, sub-rectangles, transparency,
disposal, local and global tables) and PNG reader (CRCs, zlib header and
Adler-32 checked by hand), and decode every file the tool writes again,
comparing every pixel of every frame. One test checks the GIF89a bytes by
hand. The tests on real simulator pictures use the PNG files in the
folder named by `POCKETMEDIA_SAMPLES` (by default a `probe` folder in
make.py's default work folder) and are skipped when there are none. Temporary files
go into a `.test-*` folder here, removed after each test.
