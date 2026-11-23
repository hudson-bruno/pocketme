#!/usr/bin/env python3
"""Makes the sample card folder (boards/desktop/card).

Only the games linked into the simulator (see src/games.rs). Tests that
need a game that cannot start, or a long list of games, make their own
card folder (tests/games.rs, tests/drag.rs, tests/attack.rs).

Each game is a folder in /CONSOLE/GAMES with:
  GAME.BIN  NOT a game: one line of text that says so. The home screen
            only needs the file to be there (and not empty). In the
            simulator games are linked in (owner decision D3), not loaded
            from here.
  INFO.TXT  what the home screen shows: KEY=VALUE lines (NAME, TAG,
            TILE colour as 6 hex digits, NEEDS for optional parts). A
            copy of games/<game>/INFO.TXT, which must be the same bytes.
  ART.BIN   the home tile's picture, made from games/<game>/art.txt
            with games/art/make_art.py (when the game has one). The same
            bytes games/build-chip.sh puts on the chip card; a desktop
            test compares the two.

Why GAME.BIN is text and not a game header:
  * In the simulator a game is linked in; its file is never run.
  * On the real board (or the game loader's tests) a text file is
    refused as damaged ("GAME FILE IS DAMAGED"): it does not start with
    the magic word GAME.
  * A file with a correct header and filler would be dangerous on the
    real board: the loader would run the filler as code.
A real chip file comes from games/build-chip.sh.

Run it again after adding a game, or after changing a game's INFO.TXT
or art.txt: python3 tools/make_sample_card.py
Folders on the card that are not in LINKED are removed.
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CARD = os.path.join(HERE, "..", "card")
GAMES_DIR = os.path.join(HERE, "..", "..", "..", "games")
sys.dont_write_bytecode = True  # no __pycache__ in games/art
sys.path.insert(0, os.path.join(GAMES_DIR, "art"))
import make_art  # noqa: E402

# Linked into the simulator: the game's folder in games/. Its card folder
# is the name in capitals ('-' becomes '_').
LINKED = ["bench", "camber", "drive3d", "hairpin", "kestrel", "lander", "lantern", "mercator", "padtest", "proto3d", "rooftop", "skimmer", "stardust", "tundra"]
NOT_A_GAME = (b"NOT A GAME FILE. In the simulator this game is linked in. "
              b"The chip file comes from games/build-chip.sh.\n")


def write_game(crate):
    name = crate.upper().replace("-", "_")
    src = os.path.join(GAMES_DIR, crate)
    folder = os.path.join(CARD, "CONSOLE", "GAMES", name)
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "GAME.BIN"), "wb") as f:
        f.write(NOT_A_GAME)
    shutil.copyfile(os.path.join(src, "INFO.TXT"), os.path.join(folder, "INFO.TXT"))
    art_src = os.path.join(src, "art.txt")
    art_out = os.path.join(folder, "ART.BIN")
    if os.path.exists(art_src):
        with open(art_src, encoding="utf-8") as f:
            art = make_art.parse(f.read(), art_src)
        with open(art_out, "wb") as f:
            f.write(make_art.encode(*art))
    elif os.path.exists(art_out):
        os.remove(art_out)
    return name


def main():
    kept = {write_game(crate) for crate in LINKED}
    games = os.path.join(CARD, "CONSOLE", "GAMES")
    for folder in sorted(os.listdir(games)):
        if folder not in kept:
            shutil.rmtree(os.path.join(games, folder))
            print("removed", folder)
    print("sample card written to", os.path.normpath(CARD))


main()
