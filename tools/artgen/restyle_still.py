"""Repaint a still sprite (building, object, tree) with SDXL + ControlNet, inside its silhouette.

    python restyle_still.py buildings-roman BUILDING_WOODCUTTERHUT                 # candidates
    python restyle_still.py buildings-roman BUILDING_WOODCUTTERHUT --install 1234  # into the pack

How it works, per sequence (0 construction, 1 finished, ...; frame 0 of each):
1. The original frame from the Studio's reference strip is split into body (opaque) and shadow
   (the game's 50 % black pixels). The body goes on a flat ground colour, enlarged to SDXL size.
2. SDXL repaints the body region only (a latent noise mask = the body, grown a little), guided by a
   ControlNet Union "canny" map of the original, so the outline, door and roof line stay where the
   gameplay data expects them while materials, detail and style are new. Strength and denoise set
   how far it may wander.
3. The result is scaled to the pack's size (original x pack scale), cut with the original body
   mask and laid over the original shadow. Same canvas, same anchor: no seq.json is needed.

Candidates go to the work folder (``%APPDATA%/s4rs-asset-studio/artgen-work``) with a comparison
sheet; ``--install SEED`` copies one into the Studio's pack. The pack and the work folder hold art
derived from the game's frames: keep both local.
"""

from __future__ import annotations

import argparse
import json
import random
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

sys.path.insert(0, str(Path(__file__).resolve().parent))
from artgen.comfy import Comfy  # noqa: E402
from artgen.spec import Studio, ref_frames, work_dir  # noqa: E402

HERE = Path(__file__).resolve().parent
GROUND = (96, 118, 74)  # roughly the game's grass, so the model lights the body for that ground
GEN_MIN, GEN_MAX = 1152, 2048  # SDXL's long side: at least its native size, at most where it falls apart


def gen_long_side(w: int, h: int, scale: int) -> int:
    return max(GEN_MIN, min(GEN_MAX, max(w, h) * scale))


def load_prompts(group: str, slot: str, seq_label: str, style_name: str, subject: str | None = None) -> tuple[str, str]:
    style = json.loads((HERE / "styles" / f"{style_name}.json").read_text(encoding="utf-8"))
    subjects = {}
    p = HERE / "prompts" / f"{group}.json"
    if p.is_file():
        subjects = json.loads(p.read_text(encoding="utf-8"))
    subject = subject or subjects.get(slot) or slot.split("_", 1)[-1].replace("_", " ").lower()
    extra = style.get("seq", {}).get(seq_label.split(" ", 1)[-1], "")
    pos = style["positive"].format(subject=subject)
    if extra:
        pos = f"{pos}, {extra}"
    return pos, style["negative"]


def split(frame: Image.Image) -> tuple[Image.Image, Image.Image]:
    """Body mask (opaque pixels) and shadow mask (the 50 % black pixels) as L images."""
    a = frame.getchannel("A")
    body = a.point(lambda v: 255 if v == 255 else 0)
    shadow = a.point(lambda v: 255 if 0 < v < 255 else 0)
    return body, shadow


def prepare(frame: Image.Image, body: Image.Image, grow: int, long_side: int):
    """The init image and the noise mask at generation size, plus how to undo the padding."""
    w, h = frame.size
    f = long_side / max(w, h)
    sw, sh = round(w * f), round(h * f)
    gw, gh = -(-sw // 64) * 64, -(-sh // 64) * 64
    ox, oy = (gw - sw) // 2, (gh - sh) // 2

    flat = Image.new("RGBA", (w, h), GROUND + (255,))
    rgb = frame.copy()
    rgb.putalpha(body)
    flat.alpha_composite(rgb)
    init = Image.new("RGB", (gw, gh), GROUND)
    init.paste(flat.convert("RGB").resize((sw, sh), Image.LANCZOS), (ox, oy))

    m = body.filter(ImageFilter.MaxFilter(2 * grow + 1)) if grow else body
    mask = Image.new("L", (gw, gh), 0)
    mask.paste(m.resize((sw, sh), Image.BILINEAR), (ox, oy))
    mask = mask.filter(ImageFilter.GaussianBlur(f * 0.75))
    return init, mask, (ox, oy, sw, sh)


def graph(ckpt, cnet, init_name, mask_name, pos, neg, seed, a) -> dict:
    return {
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": ckpt}},
        "2": {"class_type": "CLIPTextEncode", "inputs": {"text": pos, "clip": ["1", 1]}},
        "3": {"class_type": "CLIPTextEncode", "inputs": {"text": neg, "clip": ["1", 1]}},
        "4": {"class_type": "LoadImage", "inputs": {"image": init_name}},
        "5": {"class_type": "LoadImage", "inputs": {"image": mask_name}},
        "6": {"class_type": "ImageToMask", "inputs": {"image": ["5", 0], "channel": "red"}},
        "7": {"class_type": "Canny", "inputs": {"image": ["4", 0], "low_threshold": a.canny_low, "high_threshold": a.canny_high}},
        "8": {"class_type": "ControlNetLoader", "inputs": {"control_net_name": cnet}},
        "9": {"class_type": "SetUnionControlNetType", "inputs": {"control_net": ["8", 0], "type": "canny/lineart/anime_lineart/mlsd"}},
        "10": {"class_type": "ControlNetApplyAdvanced", "inputs": {
            "positive": ["2", 0], "negative": ["3", 0], "control_net": ["9", 0], "image": ["7", 0],
            "strength": a.control, "start_percent": 0.0, "end_percent": a.control_end, "vae": ["1", 2]}},
        "11": {"class_type": "VAEEncode", "inputs": {"pixels": ["4", 0], "vae": ["1", 2]}},
        "12": {"class_type": "SetLatentNoiseMask", "inputs": {"samples": ["11", 0], "mask": ["6", 0]}},
        "13": {"class_type": "KSampler", "inputs": {
            "model": ["1", 0], "seed": seed, "steps": a.steps, "cfg": a.cfg, "sampler_name": "dpmpp_2m",
            "scheduler": "karras", "positive": ["10", 0], "negative": ["10", 1], "latent_image": ["12", 0],
            "denoise": a.denoise}},
        "14": {"class_type": "VAEDecode", "inputs": {"samples": ["13", 0], "vae": ["1", 2]}},
        "15": {"class_type": "PreviewImage", "inputs": {"images": ["14", 0]}},
        "16": {"class_type": "PreviewImage", "inputs": {"images": ["7", 0]}},
    }


def smooth_mask(m: Image.Image, size: tuple[int, int], out: tuple[int, int]) -> Image.Image:
    """Enlarge a 1x pixel mask without its staircase: blur at high resolution, re-threshold, and
    scale down to the output with antialiased edges."""
    big = m.resize(size, Image.BICUBIC).filter(ImageFilter.GaussianBlur(size[0] / m.width * 0.45))
    big = big.point(lambda v: 255 if v >= 128 else 0)
    return big.resize(out, Image.LANCZOS)


def finish(gen: Image.Image, frame: Image.Image, body: Image.Image, shadow: Image.Image, crop, scale: int) -> Image.Image:
    ox, oy, sw, sh = crop
    w, h = frame.size
    W, H = w * scale, h * scale
    art = gen.convert("RGB").crop((ox, oy, ox + sw, oy + sh)).resize((W, H), Image.LANCZOS)
    if scale == 1:
        a_body, a_shadow = body, shadow
    else:
        a_body, a_shadow = smooth_mask(body, (sw, sh), (W, H)), smooth_mask(shadow, (sw, sh), (W, H))
    out = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    out.paste(Image.new("RGBA", (W, H), (0, 0, 0, 128)), (0, 0), a_shadow)
    top = art.convert("RGBA")
    top.putalpha(a_body)
    out.alpha_composite(top)
    return out


def on_ground(im: Image.Image, zoom: int) -> Image.Image:
    bg = Image.new("RGBA", im.size, GROUND + (255,))
    bg.alpha_composite(im)
    return bg.resize((im.width * zoom, im.height * zoom), Image.NEAREST)


def sheet(orig: Image.Image, cands: list[tuple[int, Image.Image]], path: Path):
    zoom = max(1, 400 // max(orig.size))
    tiles = [("original", on_ground(orig, zoom))] + [
        (f"seed {s}", on_ground(c.resize(orig.size, Image.LANCZOS) if c.size != orig.size else c, zoom)) for s, c in cands
    ]
    tw, th = tiles[0][1].size
    cols = min(4, len(tiles))
    rows = -(-len(tiles) // cols)
    out = Image.new("RGB", (cols * (tw + 8), rows * (th + 24)), (30, 30, 30))
    d = ImageDraw.Draw(out)
    for i, (label, t) in enumerate(tiles):
        x, y = (i % cols) * (tw + 8), (i // cols) * (th + 24)
        out.paste(t.convert("RGB"), (x, y + 20))
        d.text((x + 4, y + 4), label, fill=(230, 230, 230))
    out.save(path)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("group")
    ap.add_argument("slot")
    ap.add_argument("--seqs", default=None, help="sequence keys, comma separated (default: all)")
    ap.add_argument("--count", type=int, default=4, help="candidates to generate")
    ap.add_argument("--seed", type=int, action="append", help="explicit seed(s) instead of random ones")
    ap.add_argument("--install", type=int, metavar="SEED", help="copy this seed's candidates into the pack")
    ap.add_argument("--style", default="default", help="styles/<name>.json")
    ap.add_argument("--prompt", help="subject text instead of prompts/<group>.json")
    ap.add_argument("--denoise", type=float, default=0.8, help="0.4 restyle .. 1.0 repaint fully")
    ap.add_argument("--control", type=float, default=0.65, help="ControlNet strength (outline lock)")
    ap.add_argument("--control-end", type=float, default=0.75)
    ap.add_argument("--canny-low", type=float, default=0.12)
    ap.add_argument("--canny-high", type=float, default=0.35)
    ap.add_argument("--grow", type=int, default=2, help="px the repaint mask grows past the body")
    ap.add_argument("--steps", type=int, default=30)
    ap.add_argument("--cfg", type=float, default=6.0)
    ap.add_argument("--scale", type=int, help="output scale (default: the pack's)")
    ap.add_argument("--checkpoint", default="sd_xl_base_1.0.safetensors")
    ap.add_argument("--controlnet", default="xinsir-union-sdxl-promax.safetensors")
    ap.add_argument("--data", type=Path, help="Studio data folder")
    ap.add_argument("--keep-server", action="store_true", help="leave a ComfyUI started here running")
    a = ap.parse_args()

    studio = Studio(a.data)
    slot = studio.slot(a.group, a.slot)
    seqs = [slot.seq(k) for k in a.seqs.split(",")] if a.seqs else slot.seqs
    scale = a.scale or studio.pack_scale()
    out_dir = work_dir() / a.group / a.slot

    if a.install is not None:
        for q in seqs:
            raw = out_dir / q.key / f"seed{a.install}.raw.png"
            if not raw.is_file():
                raise SystemExit(f"no candidate {raw}")
            if len(q.frames) != 1:
                print(f"warning: sequence {q.key} has {len(q.frames)} frames; only frame 0 is replaced", file=sys.stderr)
            frame = ref_frames(studio, q)[0]
            body, shadow = split(frame)
            meta = raw.with_name(f"seed{a.install}.json")
            # candidates made before the crop was recorded were generated at GEN_MIN
            crop = json.loads(meta.read_text())["crop"] if meta.is_file() else prepare(frame, body, a.grow, GEN_MIN)[2]
            out = finish(Image.open(raw), frame, body, shadow, crop, scale)
            d = studio.write_seq(a.group, a.slot, q.key, [out])
            print(f"installed seed {a.install} at scale {scale} ({out.width}x{out.height}) -> {d}")
        return

    comfy = Comfy()
    comfy.ensure()
    try:
        generate(a, studio, slot, seqs, scale, out_dir, comfy)
    finally:
        if not a.keep_server:
            comfy.stop()


def generate(a, studio, slot, seqs, scale, out_dir, comfy):
    for need, folder in ((a.checkpoint, "checkpoints"), (a.controlnet, "controlnet")):
        if need not in comfy.models(folder):
            raise SystemExit(f"ComfyUI has no {folder}/{need}; it has {comfy.models(folder)}")

    seeds = a.seed or [random.randrange(1, 2**31) for _ in range(a.count)]
    for q in seqs:
        frame = ref_frames(studio, q)[0]
        body, shadow = split(frame)
        init, mask, crop = prepare(frame, body, a.grow, gen_long_side(*frame.size, scale))
        tag = f"{a.group}_{a.slot}_{q.key}"
        init_name = comfy.upload(init, f"{tag}_init.png")
        mask_name = comfy.upload(mask.convert("RGB"), f"{tag}_mask.png")
        pos, neg = load_prompts(a.group, a.slot, q.label, a.style, a.prompt)
        print(f"[{q.key} {q.label}] {pos}")
        d = out_dir / q.key
        d.mkdir(parents=True, exist_ok=True)
        cands = []
        for seed in seeds:
            res = comfy.run(graph(a.checkpoint, a.controlnet, init_name, mask_name, pos, neg, seed, a))
            gen = res["15"][0]
            gen.save(d / f"seed{seed}.raw.png")
            (d / f"seed{seed}.json").write_text(json.dumps({"crop": crop}), encoding="utf-8")
            if "16" in res:
                res["16"][0].save(d / "canny.png")
            out = finish(gen, frame, body, shadow, crop, scale)
            out.save(d / f"seed{seed}.png")
            cands.append((seed, out))
            print(f"  seed {seed} -> {d / f'seed{seed}.png'}")
        (d / "settings.json").write_text(json.dumps({"prompt": pos, "negative": neg, **{k: v for k, v in vars(a).items() if k not in ("data", "install")}}, indent=1, default=str), encoding="utf-8")
        sheet(frame, cands, d / "sheet.png")
        print(f"  sheet: {d / 'sheet.png'}")
    print(f"install one with: python {Path(__file__).name} {a.group} {a.slot} --install SEED")


if __name__ == "__main__":
    main()
