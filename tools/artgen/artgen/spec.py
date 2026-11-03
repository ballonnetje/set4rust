"""Read the Asset Studio's spec and write into its pack.

The spec (``assetspec/``) and the pack (``pack/``) live in the Studio's data folder. The layout is
described in ``studio/README.md``. Reference images under ``assetspec/ref`` are renders of the game's
own art: they are read here as guides and never written anywhere else.
"""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path

# Frame tuple: [image, w, h, left, top, flags, sx, sy] (index.json frame_fields).
FLAG_TEAM, FLAG_SHADOW, FLAG_HALF, FLAG_RAW = 1, 2, 4, 8

# Map geometry at zoom 1 (index.json geometry): node (x, y) -> (24x - 12y, 12y) px.
NODE_DX = 24
ROW_DY = 12

# Sprite direction d faces the sim's NEIGHBOR[d] = (1,0) (1,1) (0,1) (-1,0) (-1,-1) (0,-1):
# on screen E, SE, SW, W, NW, NE.
DIRECTION_NAMES = ["E", "SE", "SW", "W", "NW", "NE"]


def studio_data_dir() -> Path:
    env = os.environ.get("S4_STUDIO_DATA")
    if env:
        return Path(env)
    if os.name == "nt":
        return Path(os.environ["APPDATA"]) / "s4rs-asset-studio"
    if os.uname().sysname == "Darwin":  # type: ignore[attr-defined]
        return Path.home() / "Library/Application Support/s4rs-asset-studio"
    xdg = os.environ.get("XDG_DATA_HOME")
    return Path(xdg) / "s4rs-asset-studio" if xdg else Path.home() / ".local/share/s4rs-asset-studio"


@dataclass
class Frame:
    image: int
    w: int
    h: int
    left: int
    top: int
    flags: int
    sx: int
    sy: int

    @property
    def team(self) -> bool:
        return bool(self.flags & FLAG_TEAM)

    @property
    def half(self) -> bool:
        return bool(self.flags & FLAG_HALF)


@dataclass
class Seq:
    key: str
    label: str
    ref: str
    mask: str | None
    frames: list[Frame]


@dataclass
class Slot:
    group: str
    id: str
    raw: dict
    seqs: list[Seq]

    def seq(self, key: str) -> Seq:
        for s in self.seqs:
            if s.key == key:
                return s
        raise KeyError(f"{self.group}/{self.id} has no sequence {key!r} (has {[s.key for s in self.seqs]})")


class Studio:
    def __init__(self, data: Path | None = None):
        self.data = Path(data) if data else studio_data_dir()
        self.spec_dir = self.data / "assetspec"
        self.pack_dir = self.data / "pack"
        if not (self.spec_dir / "index.json").is_file():
            raise SystemExit(
                f"No asset spec in {self.spec_dir}. Start the Asset Studio once, or run "
                f"`s4-asset-studio --export-spec {self.spec_dir}`."
            )
        self.index = json.loads((self.spec_dir / "index.json").read_text(encoding="utf-8"))
        self._groups: dict[str, dict] = {}

    def group(self, gid: str) -> dict:
        if gid not in self._groups:
            p = self.spec_dir / "groups" / f"{gid}.json"
            if not p.is_file():
                names = sorted(q.stem for q in (self.spec_dir / "groups").glob("*.json"))
                raise SystemExit(f"unknown group {gid!r}; groups: {', '.join(names)}")
            self._groups[gid] = json.loads(p.read_text(encoding="utf-8"))
        return self._groups[gid]

    def slot(self, gid: str, sid: str) -> Slot:
        g = self.group(gid)
        for s in g["slots"]:
            if s["id"] == sid:
                seqs = [
                    Seq(q["key"], q["label"], q["ref"], q.get("mask"), [Frame(*f[:8]) for f in q["frames"]])
                    for q in s["seqs"]
                ]
                return Slot(gid, sid, s, seqs)
        raise SystemExit(f"group {gid} has no slot {sid!r}")

    def ref_path(self, rel: str) -> Path:
        return self.spec_dir / rel

    # --- pack -----------------------------------------------------------------------------------

    def pack_json(self) -> dict:
        p = self.pack_dir / "pack.json"
        return json.loads(p.read_text(encoding="utf-8")) if p.is_file() else {"format": 1, "scale": 1}

    def pack_scale(self) -> int:
        return int(self.pack_json().get("scale", 1))

    def seq_dir(self, gid: str, sid: str, key: str) -> Path:
        return self.pack_dir / "gfx" / gid / sid / key

    def write_seq(self, gid: str, sid: str, key: str, frames, masks=None, seq_json: dict | None = None):
        """Replace a sequence in the pack: frames are PIL RGBA images, masks optional L images."""
        d = self.seq_dir(gid, sid, key)
        d.mkdir(parents=True, exist_ok=True)
        for old in list(d.glob("*.png")):
            old.unlink()
        old_seq = d / "seq.json"
        keep = json.loads(old_seq.read_text(encoding="utf-8")) if old_seq.is_file() else {}
        for i, im in enumerate(frames):
            im.save(d / f"{i:03}.png", optimize=True)
            if masks is not None:
                masks[i].save(d / f"{i:03}.team.png", optimize=True)
        if seq_json:
            # Keep keys the Studio or the user added; drop stale anchors when ours replace them.
            for k in ("anchor", "anchors"):
                keep.pop(k, None)
            keep.update(seq_json)
        if keep:
            old_seq.write_text(json.dumps(keep), encoding="utf-8")
        elif old_seq.is_file():
            old_seq.unlink()
        return d


def ref_frames(studio: Studio, seq: Seq):
    """Cut the reference strip into one RGBA image per original frame (PIL)."""
    from PIL import Image

    strip = Image.open(studio.ref_path(seq.ref)).convert("RGBA")
    return [strip.crop((f.sx, f.sy, f.sx + f.w, f.sy + f.h)) for f in seq.frames]


def work_dir() -> Path:
    """Scratch space for candidates and previews: outside every repository."""
    env = os.environ.get("ARTGEN_WORK")
    p = Path(env) if env else studio_data_dir() / "artgen-work"
    p.mkdir(parents=True, exist_ok=True)
    return p
