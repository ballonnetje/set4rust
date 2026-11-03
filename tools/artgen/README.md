# artgen: new art into Asset Studio packs

Local tools that make **replacement art** for the Asset Studio's pack (`studio/README.md`, "The
pack"): SDXL repaints still sprites, Blender renders rigged characters. Everything they write goes to
the Studio's data folder, never into this repository.

> The pack and the work folder hold art derived from the game's own frames (the stills are repainted
> inside the original outline). Keep both local; do not publish them.

| Script | For | Engine |
| --- | --- | --- |
| `restyle_still.py` | buildings, objects, trees: one frame per sequence | ComfyUI + SDXL + ControlNet Union |
| `render_settler.py` | settler, animal and vehicle jobs: 6 directions x n frames | Blender (Cycles) |
| `blender/make_test_settler.py` | a placeholder woodcutter to test the Blender path | Blender |

## Setup (Windows, as installed on 2026-11-03)

* **ComfyUI Desktop** (already installed): models in `Documents\ComfyUI\models`. `restyle_still.py`
  starts its own headless server from the Desktop's venv on port 8189 and stops it afterwards
  (`--keep-server` leaves it up for quicker runs). Point `COMFY_URL` at a running server instead,
  or set `COMFY_PYTHON`, `COMFY_MAIN`, `COMFY_BASE` for a non-Desktop install.
* **Models** (both licences allow commercial use):
  * `checkpoints/sd_xl_base_1.0.safetensors`: Stability AI SDXL 1.0 base, CreativeML Open RAIL++-M
  * `controlnet/xinsir-union-sdxl-promax.safetensors`: xinsir ControlNet Union SDXL ProMax, Apache 2.0
* **Blender** 5.2 (Steam). Set `BLENDER` if it lives elsewhere. Cycles uses OptiX on the RTX card.
* **Python** 3.11+ with Pillow, for the scripts themselves.

## Stills: `restyle_still.py`

```text
python restyle_still.py buildings-roman BUILDING_WOODCUTTERHUT              # 4 candidates per sequence
python restyle_still.py buildings-roman BUILDING_WOODCUTTERHUT --install SEED
```

Per sequence (0 construction, 1 finished): the original frame is split into body and shadow. SDXL
repaints the body only (a latent noise mask), held to the original outline by a canny ControlNet.
The result is cut with the original body mask and laid over the original shadow, at the pack's
scale. Same canvas and anchor, so the building's cells (door, flag, footprint) still fit.

* Candidates, raw 1152 px outputs, the canny map and a comparison sheet go to
  `%APPDATA%\s4rs-asset-studio\artgen-work\<group>\<slot>\<seq>\`.
* Use the same seed for construction and finished, so the pair matches.
* `--denoise` (0.8) sets how much is repainted (0.5 restyle .. 1.0 new). `--control` (0.65) sets how
  tightly it follows the original's lines.
* Prompts: `styles/<name>.json` holds the shared look; `prompts/<group>.json` describes each slot.
  Slots without an entry use their id.
* Output size is original x the pack's `scale` (set to 4 on 2026-11-03). SDXL generates at that
  size or more (long side 1152..2048 px). Above 2048 px the result is enlarged, not more detailed.
  Outlines and shadows are smoothed at generation size, so they don't show the 1x staircase.
* `--install SEED` rebuilds the candidate from its raw output at the current scale. After changing
  the scale, re-install; there is no need to generate again.
* Animated building parts (sequences 2+) and multi-frame objects are not handled: the script replaces
  frame 0 only and warns. Repainting them frame by frame would flicker.

## Characters: `render_settler.py`

```text
python render_settler.py settlers-roman WC_WALK WC_CUT_TREE --blend woodcutter.blend
python render_settler.py settlers-roman WC_WALK --blend woodcutter.blend --mult 2   # 2 frames per original
```

Renders are made at the pack's scale (24 x scale px per node), so a scale change needs a re-render.

For each job: the original frame count per direction comes from the spec (the simulation times jobs
by it), and Blender renders `count x mult` frames in all six directions. The frames are cropped per
direction and written with team masks and `seq.json` (anchor, frame multiplier) into the pack. A
comparison sheet (original vs new per direction, anchor marked, team colour applied) goes to the work
folder. `--no-install` only renders and compares.

What a character `.blend` must hold (`blender/render_sprites.py` explains it in full):

* one armature that every part is parented to, standing on the anchor node at the origin, facing -Y,
  1 Blender unit = 1 node spacing. The game's settlers are about **2.1 units** tall;
* one action per job, named like the job (`WC_WALK`), whose frame range is one loop with the last
  frame equal to the first (`--action SLOT=ACTION` maps other names);
* team-colour materials named `team*` or with the custom property `team = 1`, painted light grey.

The script adds the camera, light and ground itself:

* **Camera:** orthographic at the true isometric pitch, 35.264° (the game's 12 px row step against
  its 24 px node step), at 24 px per node x scale. The script checks the lattice lands where the game
  draws it.
* **Directions:** direction d faces the sim's neighbour d, which on screen is E, SE, SW, W, NW, NE.
* **Light:** sun from the upper left. The shadow is caught in a pass of its own and flattened to
  black at `--shadow` 0.55 opacity, like the game's 50 % shadows.

`make_test_settler.py` builds a primitive-shape woodcutter with both actions. It shows that the
pipeline is right, not what the art should look like:

```text
blender -b --factory-startup --python blender/make_test_settler.py -- %APPDATA%\s4rs-asset-studio\artgen-work\characters\test_woodcutter.blend
```

## Still missing

* A real character model. Paths: model and rig it in Blender (by hand or by volunteers), or SD
  concept art → image-to-3D (TRELLIS, MIT; Hunyuan3D's licence excludes the EU) → auto-rig →
  animate.
* A style LoRA trained on approved results, so a whole race looks like one set.
* The game does not load packs yet; until then the Studio is where the results are seen.
