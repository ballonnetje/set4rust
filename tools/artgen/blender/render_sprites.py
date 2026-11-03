"""Render one action of a rigged character as game sprites (run inside Blender; see render_settler.py).

    blender -b CHARACTER.blend --python render_sprites.py -- --out DIR --action WC_WALK --count 13 ...

The scene contract (what a character .blend must hold):
* one armature (or the object named by --rig) that everything of the character is parented to,
  standing on its anchor node at the world origin, facing -Y, 1 Blender unit = 1 map node spacing;
* an action per game job, named like the job (``WC_WALK``), whose frame range is one loop (the last
  frame equals the first) or the whole one-shot animation;
* materials of team-colour parts named ``team*`` or with the custom property ``team`` = 1, painted
  light grey (the game multiplies the player's colour by their brightness).

The script adds its own camera, sun, ambient light and shadow catcher, so the .blend holds none.
Camera: orthographic, true isometric pitch (35.264 deg, from the game's 24 px node step against its
12 px row step), 24 px per node x scale. Direction d turns the rig to face the sim's neighbour d:
E, SE, SW, W, NW, NE. Two passes per frame: the beauty image (Cycles, transparent film, the shadow
caught on the ground as alpha, in a pass of its own) and the team mask (team parts white, everything else black).
"""

import argparse
import json
import math
import sys
from pathlib import Path

import bpy
from bpy_extras.object_utils import world_to_camera_view
from mathutils import Matrix, Vector

PITCH = math.asin(1 / math.sqrt(3))  # 35.264 deg: 12 px row step vs 24 px node step


def parse():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--action", required=True)
    ap.add_argument("--count", type=int, required=True, help="frames to render per direction")
    ap.add_argument("--dirs", default="0,1,2,3,4,5")
    ap.add_argument("--loop", type=int, default=1, help="1: the action loops (last frame = first)")
    ap.add_argument("--px", type=float, default=24.0, help="pixels per node (24 x scale)")
    ap.add_argument("--canvas", type=int, nargs=2, default=[128, 128])
    ap.add_argument("--anchor", type=int, nargs=2, default=[64, 88])
    ap.add_argument("--rig", default=None)
    ap.add_argument("--samples", type=int, default=64)
    ap.add_argument("--sun-elevation", type=float, default=66.0)
    ap.add_argument("--sun-azimuth", type=float, default=-20.0, help="deg; shadow direction on the ground, 0 = east, - = toward the viewer")
    ap.add_argument("--ambient", type=float, default=0.35)
    ap.add_argument("--sun", type=float, default=3.5)
    ap.add_argument("--no-mask", action="store_true")
    return ap.parse_args(argv)


def find_rig(name):
    if name:
        return bpy.data.objects[name]
    arms = [o for o in bpy.context.scene.objects if o.type == "ARMATURE"]
    if len(arms) != 1:
        raise SystemExit(f"need exactly one armature or --rig, found {[o.name for o in arms]}")
    return arms[0]


def setup_cycles(scene, samples):
    scene.render.engine = "CYCLES"
    prefs = bpy.context.preferences.addons["cycles"].preferences
    for kind in ("OPTIX", "CUDA", "HIP", "METAL", "ONEAPI"):
        try:
            prefs.compute_device_type = kind
        except TypeError:
            continue
        prefs.get_devices()
        devs = [d for d in prefs.devices if d.type == kind]
        if devs:
            for d in prefs.devices:
                d.use = d.type == kind
            scene.cycles.device = "GPU"
            print(f"cycles: {kind} {[d.name for d in devs]}")
            break
    else:
        scene.cycles.device = "CPU"
        print("cycles: CPU")
    scene.cycles.samples = samples
    scene.cycles.use_denoising = True
    scene.cycles.pixel_filter_type = "BLACKMAN_HARRIS"
    scene.cycles.filter_width = 1.2
    scene.render.film_transparent = True
    scene.render.use_persistent_data = True
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"
    scene.render.image_settings.color_depth = "8"
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"


def setup_camera(scene, a):
    W, H = a.canvas
    ax, ay = a.anchor
    cam_data = bpy.data.cameras.new("artgen_cam")
    cam_data.type = "ORTHO"
    big = max(W, H)
    cam_data.ortho_scale = big / a.px
    # Put the world origin (the anchor node) on pixel (ax, ay) of the canvas.
    cam_data.shift_x = -(ax - W / 2) / big
    cam_data.shift_y = (ay - H / 2) / big
    cam_data.clip_start = 0.1
    cam_data.clip_end = 500
    cam = bpy.data.objects.new("artgen_cam", cam_data)
    scene.collection.objects.link(cam)
    d = 100.0
    cam.location = (0.0, -d * math.cos(PITCH), d * math.sin(PITCH))
    cam.rotation_euler = (math.pi / 2 - PITCH, 0.0, 0.0)
    scene.camera = cam
    scene.render.resolution_x = W
    scene.render.resolution_y = H
    scene.render.resolution_percentage = 100
    scene.render.pixel_aspect_x = scene.render.pixel_aspect_y = 1
    bpy.context.view_layer.update()

    def px(p):
        v = world_to_camera_view(scene, cam, Vector(p))
        return (v.x * W, (1 - v.y) * H)

    # The map lattice must land where the game draws it: node (1,0) 24 px right, node (0,1)
    # (world (-0.5, -0.866)) 12 px left and 12 px down, height 0 on the anchor.
    s = a.px / 24.0
    for world, want in (((0, 0, 0), (ax, ay)), ((1, 0, 0), (ax + 24 * s, ay)),
                        ((-0.5, -math.sqrt(3) / 2, 0), (ax - 12 * s, ay + 12 * s))):
        got = px(world)
        if abs(got[0] - want[0]) > 0.01 or abs(got[1] - want[1]) > 0.01:
            raise SystemExit(f"camera check failed: {world} -> {got}, want {want}")
    return cam


def setup_lights(scene, a):
    sun_data = bpy.data.lights.new("artgen_sun", "SUN")
    sun_data.energy = a.sun
    sun_data.angle = math.radians(2.0)
    sun = bpy.data.objects.new("artgen_sun", sun_data)
    scene.collection.objects.link(sun)
    el, az = math.radians(a.sun_elevation), math.radians(a.sun_azimuth)
    travel = Vector((math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), -math.sin(el)))
    sun.rotation_euler = (-travel).to_track_quat("Z", "Y").to_euler()

    world = bpy.data.worlds.new("artgen_world")
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    bg.inputs["Color"].default_value = (1, 1, 1, 1)
    bg.inputs["Strength"].default_value = a.ambient
    scene.world = world

    bpy.ops.mesh.primitive_plane_add(size=60, location=(0, 0, 0))
    ground = bpy.context.active_object
    ground.name = "artgen_ground"
    ground.is_shadow_catcher = True
    ground.visible_glossy = False
    return sun, world, ground


def is_team(mat):
    return mat is not None and (mat.name.lower().startswith("team") or bool(mat.get("team", 0)))


def emission(name, value):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Color"].default_value = (value, value, value, 1)
    em.inputs["Strength"].default_value = 1.0
    nt.links.new(em.outputs[0], out.inputs[0])
    return m


def character_objects(rig):
    out = [rig]
    stack = list(rig.children)
    while stack:
        o = stack.pop()
        out.append(o)
        stack.extend(o.children)
    return out


def main():
    a = parse()
    scene = bpy.context.scene
    for o in list(scene.objects):  # stray cameras and lights of the .blend would change the look
        if o.type in ("CAMERA", "LIGHT"):
            o.hide_render = True
    rig = find_rig(a.rig)
    act = bpy.data.actions.get(a.action)
    if act is None:
        raise SystemExit(f"no action {a.action!r}; actions: {[x.name for x in bpy.data.actions]}")
    rig.animation_data_create()
    rig.animation_data.action = act
    if hasattr(act, "slots") and len(act.slots) and rig.animation_data.action_slot is None:
        rig.animation_data.action_slot = act.slots[0]

    setup_cycles(scene, a.samples)
    setup_camera(scene, a)
    sun, world, ground = setup_lights(scene, a)

    f0, f1 = act.frame_range
    span = f1 - f0
    times = [f0 + span * i / a.count for i in range(a.count)] if a.loop else \
        [f0 + span * i / max(1, a.count - 1) for i in range(a.count)]
    dirs = [int(d) for d in a.dirs.split(",")]
    out = Path(a.out)
    (out / "beauty").mkdir(parents=True, exist_ok=True)
    for sub in ("shadow", "mask"):
        (out / sub).mkdir(parents=True, exist_ok=True)

    base_rot = rig.rotation_euler.copy()
    rig.rotation_mode = "XYZ"

    def render_all(sub):
        for d in dirs:
            rig.rotation_euler = (base_rot.x, base_rot.y, base_rot.z + math.radians(90 - 60 * d))
            for i, t in enumerate(times):
                fi = math.floor(t)
                scene.frame_set(fi, subframe=t - fi)
                scene.render.filepath = str(out / sub / f"d{d}_{i:03}.png")
                bpy.ops.render.render(write_still=True)
            print(f"{sub}: direction {d} done", flush=True)

    # The character alone (no ground), then the ground's caught shadow: kept apart so the shadow
    # can be made flat and light like the game's, and the catcher's noise stays off the body.
    ground.hide_render = True
    render_all("beauty")
    ground.hide_render = False
    render_all("shadow")

    if not a.no_mask:
        white, black = emission("artgen_team", 1.0), emission("artgen_other", 0.0)
        found = False
        for o in character_objects(rig):
            for slot in getattr(o, "material_slots", []):
                t = is_team(slot.material)
                found |= t
                slot.link = "OBJECT"
                slot.material = white if t else black
        if not found:
            print("warning: no team material (name team* or property team=1); the mask is empty")
        ground.hide_render = True
        world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.0
        sun.hide_render = True
        scene.cycles.samples = 8
        scene.cycles.use_denoising = False
        render_all("mask")

    (out / "render.json").write_text(json.dumps({
        "action": a.action, "count": a.count, "dirs": dirs, "canvas": a.canvas, "anchor": a.anchor,
        "px": a.px, "times": times, "mask": not a.no_mask,
    }), encoding="utf-8")


main()
