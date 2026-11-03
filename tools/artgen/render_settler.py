"""Render a rigged character's actions into the Studio's pack as settler, animal or vehicle jobs.

    python render_settler.py settlers-roman WC_WALK WC_CUT_TREE --blend woodcutter.blend
    python render_settler.py settlers-roman WC_WALK --blend woodcutter.blend --mult 2

For each job slot: read the original's frame count per direction from the spec (the simulation
times jobs by it), render count x multiplier frames in all six directions with
``blender/render_sprites.py``, crop each direction to what was drawn, and write the frames, team
masks and ``seq.json`` (anchor, frame multiplier) into ``pack/gfx/<group>/<slot>/<dir>/``. A
comparison sheet (original vs new, per direction, with the anchor marked) goes to the work folder.

The action in the .blend has the slot's name unless ``--action SLOT=ACTION`` maps it.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
from artgen.spec import DIRECTION_NAMES, Studio, ref_frames, work_dir  # noqa: E402

HERE = Path(__file__).resolve().parent
GROUND = (96, 118, 74)


def find_blender() -> str:
    env = os.environ.get("BLENDER")
    if env:
        return env
    for p in (
        r"C:\Program Files (x86)\Steam\steamapps\common\Blender\blender.exe",
        r"C:\Program Files\Blender Foundation\Blender 5.2\blender.exe",
        "/Applications/Blender.app/Contents/MacOS/Blender",
        "/usr/bin/blender",
    ):
        if Path(p).is_file():
            return p
    raise SystemExit("Blender not found; set BLENDER to its executable")


def with_shadow(body: Image.Image, shadow_pass: Image.Image, strength: float, floor: int) -> Image.Image:
    """The character over its caught shadow, flattened to plain black at ``strength`` opacity."""
    sa = shadow_pass.getchannel("A").point(lambda v: 0 if v < floor else min(255, round(v * strength)))
    out = Image.new("RGBA", body.size, (0, 0, 0, 0))
    out.putalpha(sa)
    out.alpha_composite(body)
    return out


def tint(im: Image.Image, mask: Image.Image | None, rgb) -> Image.Image:
    """The game's team colour: mix(rgb, player x luma(rgb), mask)."""
    if mask is None:
        return im
    luma = im.convert("L")
    col = Image.merge("RGB", [luma.point(lambda v, c=c: v * c // 255) for c in rgb])
    out = Image.composite(col.convert("RGBA"), im, mask)
    out.putalpha(im.getchannel("A"))
    return out


def bbox_union(images):
    box = None
    for im in images:
        b = im.getchannel("A").getbbox()
        if b:
            box = b if box is None else (min(box[0], b[0]), min(box[1], b[1]), max(box[2], b[2]), max(box[3], b[3]))
    return box


def team_mask(mask_rgba: Image.Image) -> Image.Image:
    r, _, _, a = mask_rgba.split()
    # straight-alpha PNG: team parts render white; drop pixels the mask pass did not cover
    return Image.composite(r, Image.new("L", r.size, 0), a.point(lambda v: 255 if v > 0 else 0))


def compare_sheet(studio, slot, new_frames, new_masks, anchors, scale: int, path: Path):
    zoom = 4 if scale == 1 else 2
    cells = []
    for q in slot.seqs:
        d = int(q.key)
        if d not in new_frames:
            continue
        o = ref_frames(studio, q)[0]
        f = q.frames[0]
        n = tint(new_frames[d][0], new_masks[d][0] if new_masks.get(d) else None, studio.index["ref_team_rgb"])
        cells.append((d, o, (f.left, f.top), n, anchors[d]))
    cw = max(max(o.width * scale, n.width) for _, o, _, n, _ in cells) * zoom + 8
    ch = max(max(o.height * scale, n.height) for _, o, _, n, _ in cells) * zoom + 8
    out = Image.new("RGB", (cw * len(cells), ch * 2 + 20), (30, 30, 30))
    dr = ImageDraw.Draw(out)
    for i, (d, o, oa, n, na) in enumerate(cells):
        for row, (im, (ax, ay), z) in enumerate(((o, oa, zoom * scale), (n, na, zoom))):
            bg = Image.new("RGBA", im.size, GROUND + (255,))
            bg.alpha_composite(im)
            big = bg.resize((im.width * z, im.height * z), Image.NEAREST)
            x, y = i * cw + 4, 20 + row * ch
            out.paste(big.convert("RGB"), (x, y))
            dr.ellipse((x + ax * z - 3, y + ay * z - 3, x + ax * z + 3, y + ay * z + 3), outline=(255, 0, 0), width=2)
        dr.text((i * cw + 6, 4), f"dir {d} {DIRECTION_NAMES[d]}", fill=(230, 230, 230))
    out.save(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("group")
    ap.add_argument("slots", nargs="+")
    ap.add_argument("--blend", required=True, type=Path)
    ap.add_argument("--action", action="append", default=[], help="SLOT=ACTION when the names differ")
    ap.add_argument("--mult", type=int, default=1, help="frames per original frame (1..8)")
    ap.add_argument("--scale", type=int, help="pixel scale (default: the pack's)")
    ap.add_argument("--dirs", default="0,1,2,3,4,5")
    ap.add_argument("--samples", type=int, default=64)
    ap.add_argument("--canvas", type=int, nargs=2, default=[128, 128], help="render canvas at scale 1")
    ap.add_argument("--anchor", type=int, nargs=2, default=[64, 88], help="anchor in that canvas")
    ap.add_argument("--alpha-floor", type=int, default=24, help="caught shadow below this alpha is dropped")
    ap.add_argument("--shadow", type=float, default=0.55, help="shadow opacity (the game's is 0.5)")
    ap.add_argument("--no-install", action="store_true", help="render and compare only")
    ap.add_argument("--data", type=Path)
    a = ap.parse_args()

    studio = Studio(a.data)
    scale = a.scale or studio.pack_scale()
    names = dict(x.split("=", 1) for x in a.action)
    blender = find_blender()
    group = studio.group(a.group)
    team = bool(group.get("team"))
    dirs = [int(d) for d in a.dirs.split(",")]

    for sid in a.slots:
        slot = studio.slot(a.group, sid)
        counts = {int(q.key): len(q.frames) for q in slot.seqs}
        if len(set(counts.values())) != 1:
            print(f"note: {sid} frame counts differ per direction: {counts}")
        count = counts[dirs[0]] * a.mult
        out = work_dir() / a.group / sid / "render"
        cmd = [blender, "-b", str(a.blend), "--python", str(HERE / "blender" / "render_sprites.py"), "--",
               "--out", str(out), "--action", names.get(sid, sid), "--count", str(count), "--dirs", a.dirs,
               "--px", str(24 * scale), "--canvas", str(a.canvas[0] * scale), str(a.canvas[1] * scale),
               "--anchor", str(a.anchor[0] * scale), str(a.anchor[1] * scale), "--samples", str(a.samples)]
        if not team:
            cmd.append("--no-mask")
        print(f"[{sid}] rendering {count} frames x {len(dirs)} directions", flush=True)
        res = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
        log = out / "blender.log"
        out.mkdir(parents=True, exist_ok=True)
        log.write_text(res.stdout + res.stderr, encoding="utf-8")
        if res.returncode != 0 or not (out / "render.json").is_file():
            tail = "\n".join((res.stdout + res.stderr).splitlines()[-25:])
            raise SystemExit(f"Blender failed ({res.returncode}); log {log}\n{tail}")

        new_frames, new_masks, anchors = {}, {}, {}
        for d in dirs:
            if counts.get(d, 0) * a.mult != count:
                raise SystemExit(f"{sid} direction {d}: original has {counts.get(d)} frames, rendered {count}")
            beauty = [
                with_shadow(Image.open(out / "beauty" / f"d{d}_{i:03}.png").convert("RGBA"),
                            Image.open(out / "shadow" / f"d{d}_{i:03}.png").convert("RGBA"), a.shadow, a.alpha_floor)
                for i in range(count)
            ]
            masks = [team_mask(Image.open(out / "mask" / f"d{d}_{i:03}.png").convert("RGBA")) for i in range(count)] if team else None
            box = bbox_union(beauty)
            if box is None:
                raise SystemExit(f"{sid} direction {d}: nothing rendered (is the character at the origin?)")
            x0, y0 = max(0, box[0] - 1), max(0, box[1] - 1)
            x1, y1 = min(beauty[0].width, box[2] + 1), min(beauty[0].height, box[3] + 1)
            if box[0] == 0 or box[1] == 0 or box[2] == beauty[0].width or box[3] == beauty[0].height:
                print(f"warning: {sid} dir {d} touches the canvas edge; raise --canvas/--anchor")
            frames = [im.crop((x0, y0, x1, y1)) for im in beauty]
            mcrop = [m.crop((x0, y0, x1, y1)) for m in masks] if masks else None
            anchor = (a.anchor[0] * scale - x0, a.anchor[1] * scale - y0)
            new_frames[d], new_masks[d], anchors[d] = frames, mcrop, anchor
            if not a.no_install:
                seq = {"anchor": list(anchor)}
                if a.mult > 1:
                    seq["frame_multiplier"] = a.mult
                studio.write_seq(a.group, sid, str(d), frames, mcrop, seq)
        sheet = work_dir() / a.group / sid / "compare.png"
        compare_sheet(studio, slot, new_frames, new_masks, anchors, scale, sheet)
        where = "pack" if not a.no_install else "work folder only"
        print(f"[{sid}] {count} frames x {len(dirs)} dirs -> {where}; compare: {sheet}")


if __name__ == "__main__":
    main()
