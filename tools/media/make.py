#!/usr/bin/env python3
"""Rebuild the documentation's pictures and clips from the scene files.

Each scene in scenes/*.json names an input script for the simulator, the
frames to keep as still pictures and the frame ranges of its clips. This runs
the simulator for each scene (no window; the simulator is deterministic,
so the same scene always gives the same pictures) and writes the results
with pocketmedia:

    python3 -I tools/media/make.py --sim PATH/TO/pocketme-sim [SCENE ...]

Stills go to docs/media/<name>.png, clips to docs/media/<name>.gif, and
docs/gallery.md is written again from the clips' captions (the gallery
shows clips only; the stills are for the other pages). The raw frames go to
a work folder (default .local/media-work, never into the repository).
With --reuse, a scene whose raw frames are already in the work folder is
not run again.
"""

import argparse
import html
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import pocketmedia  # noqa: E402

GROUPS = ["system.json", "racing.json", "space.json", "arcade.json"]

# How the gallery page orders and names things.
TITLES = [
    ("console", "The console"),
    ("camber", "CAMBER"),
    ("mercator", "MERCATOR"),
    ("kestrel", "KESTREL"),
    ("lantern", "LANTERN"),
    ("drive3d", "DRIVE3D"),
    ("proto3d", "PROTO3D"),
    ("hairpin", "HAIRPIN"),
    ("rooftop", "ROOFTOP"),
    ("lander", "LANDER"),
    ("skimmer", "SKIMMER"),
    ("tundra", "TUNDRA"),
    ("stardust", "STARDUST"),
    ("bench", "BENCH"),
    ("padtest", "PADTEST"),
]


def load_scenes():
    scenes = []
    for group in GROUPS:
        with open(os.path.join(HERE, "scenes", group)) as f:
            for scene in json.load(f):
                scene["script"] = os.path.basename(scene["script"])
                scene["game"] = scene["game"].lower()
                scenes.append(scene)
    return scenes


def clips_of(scene):
    """A scene's clips: the list "clips", or the one in "clip" (older form)."""
    clips = list(scene.get("clips") or [])
    if scene.get("clip"):
        clips.insert(0, scene["clip"])
    return clips


def wanted_frames(scene):
    frames = {still["frame"] for still in scene.get("stills") or []}
    for clip in clips_of(scene):
        frames.update(range(clip["from"], clip["to"] + 1, clip["every"]))
    return sorted(frames)


def frame_path(work, number):
    return os.path.join(work, "frame-%05d.png" % number)


def run_scene(sim, scene, work):
    os.makedirs(work, exist_ok=True)
    shots = ",".join(str(n) for n in wanted_frames(scene))
    command = [
        sim, "--quiet",
        "--boot", scene["boot"],
        "--fps", str(scene["fps"]),
        "--frames", str(scene["frames"]),
        "--script", os.path.join(HERE, "scenes", scene["script"]),
        "--shot", shots,
        "--out", work,
    ]
    done = subprocess.run(
        command, cwd=os.path.join(ROOT, "boards", "desktop"),
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if done.returncode != 0:
        tail = "\n".join(done.stdout.splitlines()[-5:])
        raise SystemExit("scene %s: the simulator ended with %d\n%s"
                         % (scene["scene"], done.returncode, tail))


def build_scene(scene, work, out, scale):
    """Returns the list of (kind, name, caption, bytes) written."""
    written = []
    for still in scene.get("stills") or []:
        image = pocketmedia.read_png(frame_path(work, still["frame"]))
        data = pocketmedia.encode_png(pocketmedia.scale_image(image, scale))
        path = os.path.join(out, still["name"] + ".png")
        pocketmedia.write_file(path, data)
        written.append(("png", still["name"], still["caption"], len(data)))
    for clip in clips_of(scene):
        numbers = list(range(clip["from"], clip["to"] + 1, clip["every"]))
        frames = [pocketmedia.read_png(frame_path(work, n)) for n in numbers]
        delays = pocketmedia.frame_delays(len(frames), clip["every"], scene["fps"])
        result = pocketmedia.encode_gif(
            frames, delays, scale=scale, loop=0, hold_last=clip.get("hold", 80),
            warn=lambda text: print("   note:", text, file=sys.stderr))
        path = os.path.join(out, clip["name"] + ".gif")
        pocketmedia.write_file(path, result.data)
        written.append(("gif", clip["name"], clip["caption"], len(result.data)))
    return written


def write_gallery(scenes, results, path):
    by_game = {}
    for scene in scenes:
        clips = [item for item in results.get(scene["scene"], []) if item[0] == "gif"]
        by_game.setdefault(scene["game"], []).extend(clips)
    lines = [
        "# Gallery",
        "",
        "Every clip here was made by the simulator",
        "([boards/desktop](../boards/desktop/README.md)) running the same console",
        "and game code as the chip, from the input scripts in",
        "[tools/media/scenes](../tools/media/scenes). They show what the console",
        "draws, pixel for pixel; they are not photographs of the board, and the",
        "simulator says nothing about speed. The games are played by a script,",
        "not a person, so a clip says nothing about how hard a game is either.",
        "`python3 -I tools/media/make.py --sim <built pocketme-sim>` builds them again.",
        "",
    ]
    lines += ["- [%s](#%s)" % (title, title.lower().replace(" ", "-"))
              for game, title in TITLES if by_game.get(game)]
    for game, title in TITLES:
        items = by_game.get(game)
        if not items:
            continue
        lines += ["", "## " + title, ""]
        if game != "console":
            lines += ["More about this game: [games/%s](../games/%s/README.md)." % (game, game), ""]
        lines += ["<table>"]
        for start in range(0, len(items), 2):
            lines += ["  <tr>"]
            for kind, name, caption, _size in items[start:start + 2]:
                lines += ['    <td width="50%%" valign="top"><img src="media/%s.%s" width="400" alt="%s"><br>%s</td>'
                          % (name, kind, html.escape(caption, quote=True), html.escape(caption, quote=False))]
            lines += ["  </tr>"]
        lines += ["</table>"]
    with open(path, "w") as f:
        f.write("\n".join(lines).rstrip() + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("scenes", nargs="*", help="scene names (default: all)")
    parser.add_argument("--sim", help="the built simulator (pocketme-sim)")
    parser.add_argument("--work", default=os.path.join(ROOT, ".local", "media-work"))
    parser.add_argument("--out", default=os.path.join(ROOT, "docs", "media"))
    parser.add_argument("--scale", type=int, default=2)
    parser.add_argument("--reuse", action="store_true",
                        help="do not run a scene whose raw frames are all there")
    args = parser.parse_args()
    if args.sim:
        # The simulator is started from boards/desktop, so a path given
        # from the caller's folder must be made absolute first.
        args.sim = os.path.abspath(args.sim)

    scenes = load_scenes()
    names = [scene["scene"] for scene in scenes]
    for name in args.scenes:
        if name not in names:
            raise SystemExit("no scene named %s (there are: %s)" % (name, " ".join(names)))
    os.makedirs(args.out, exist_ok=True)

    results = {}
    total = 0
    for scene in scenes:
        if args.scenes and scene["scene"] not in args.scenes:
            continue
        work = os.path.join(args.work, scene["scene"])
        have = all(os.path.exists(frame_path(work, n)) for n in wanted_frames(scene))
        if not (args.reuse and have):
            if not args.sim:
                raise SystemExit("scene %s needs the simulator: give --sim" % scene["scene"])
            run_scene(args.sim, scene, work)
        results[scene["scene"]] = build_scene(scene, work, args.out, args.scale)
        for kind, name, _caption, size in results[scene["scene"]]:
            total += size
            print("%-18s %s.%s  %d bytes" % (scene["scene"], name, kind, size))
    print("total %d bytes" % total)
    if not args.scenes:
        write_gallery(scenes, results, os.path.join(ROOT, "docs", "gallery.md"))
        print("wrote docs/gallery.md")


if __name__ == "__main__":
    main()
