"""Build a placeholder woodcutter that meets render_sprites.py's contract (run inside Blender).

    blender -b --factory-startup --python make_test_settler.py -- OUT.blend

Primitive shapes rigidly parented to a 14-bone armature, two looping actions: WC_WALK and
WC_CUT_TREE. It exists to test the pipeline (camera, directions, frame counts, team mask, shadow,
anchor), not as art: a real character replaces it with the same bone-free contract (an armature,
actions named after the jobs, team materials).

Size: the game's settlers are about 42 px tall above the anchor at 24 px per node; at the true
isometric pitch one unit of height is 24 * cos(35.26) = 19.6 px, so a settler is ~2.1 units tall.
"""

import math
import sys

import bpy
from mathutils import Matrix, Vector

OUT = sys.argv[sys.argv.index("--") + 1] if "--" in sys.argv else "test_settler.blend"

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene


def mat(name, rgb, team=False, rough=0.7, metal=0.0):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    p = m.node_tree.nodes["Principled BSDF"]
    p.inputs["Base Color"].default_value = (*rgb, 1)
    p.inputs["Roughness"].default_value = rough
    p.inputs["Metallic"].default_value = metal
    if team:
        m["team"] = 1
    return m


SKIN = mat("skin", (0.80, 0.55, 0.40))
TEAM = mat("team_shirt", (0.80, 0.80, 0.80), team=True)
VEST = mat("vest", (0.30, 0.16, 0.07))
TROUSERS = mat("trousers", (0.50, 0.38, 0.26))
BOOTS = mat("boots", (0.22, 0.12, 0.05))
HAIR = mat("hair", (0.10, 0.06, 0.03))
WOOD = mat("axe_handle", (0.45, 0.25, 0.10))
STEEL = mat("axe_head", (0.55, 0.57, 0.60), rough=0.35, metal=0.9)

# --- armature: (name, head, tail, parent) in rest pose, character facing -Y, left = +X -----------
BONES = [
    ("root", (0, 0, 0), (0, 0.3, 0), None),
    ("hips", (0, 0, 0.95), (0, 0, 1.15), "root"),
    ("spine", (0, 0, 1.15), (0, 0, 1.58), "hips"),
    ("head", (0, 0, 1.58), (0, 0, 2.1), "spine"),
    ("upperarm.L", (0.34, 0, 1.52), (0.36, 0, 1.12), "spine"),
    ("forearm.L", (0.36, 0, 1.12), (0.36, -0.02, 0.76), "upperarm.L"),
    ("upperarm.R", (-0.34, 0, 1.52), (-0.36, 0, 1.12), "spine"),
    ("forearm.R", (-0.36, 0, 1.12), (-0.36, -0.02, 0.76), "upperarm.R"),
    ("thigh.L", (0.14, 0, 0.95), (0.14, 0, 0.52), "hips"),
    ("shin.L", (0.14, 0, 0.52), (0.14, 0.01, 0.1), "thigh.L"),
    ("thigh.R", (-0.14, 0, 0.95), (-0.14, 0, 0.52), "hips"),
    ("shin.R", (-0.14, 0, 0.52), (-0.14, 0.01, 0.1), "thigh.R"),
]

arm_data = bpy.data.armatures.new("woodcutter_rig")
rig = bpy.data.objects.new("woodcutter", arm_data)
scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
rig.select_set(True)
bpy.ops.object.mode_set(mode="EDIT")
for name, head, tail, parent in BONES:
    b = arm_data.edit_bones.new(name)
    b.head, b.tail = head, tail
    b.roll = 0.0
    if parent:
        b.parent = arm_data.edit_bones[parent]
        b.use_connect = False
bpy.ops.object.mode_set(mode="OBJECT")


def attach(obj, bone):
    """Parent obj rigidly to a bone, keeping its world placement."""
    b = arm_data.bones[bone]
    obj.parent = rig
    obj.parent_type = "BONE"
    obj.parent_bone = bone
    obj.matrix_parent_inverse = (rig.matrix_world @ b.matrix_local @ Matrix.Translation((0, b.length, 0))).inverted()


def part(kind, bone, material, loc, scale, rot=(0, 0, 0)):
    if kind == "sphere":
        bpy.ops.mesh.primitive_uv_sphere_add(segments=24, ring_count=12, location=loc, rotation=rot)
    elif kind == "cyl":
        bpy.ops.mesh.primitive_cylinder_add(vertices=20, location=loc, rotation=rot)
    else:
        bpy.ops.mesh.primitive_cube_add(location=loc, rotation=rot)
    o = bpy.context.active_object
    o.scale = scale
    o.data.materials.append(material)
    bpy.ops.object.shade_smooth()
    attach(o, bone)
    return o


def limb(bone, material, radius):
    b = arm_data.bones[bone]
    h, t = b.head_local, b.tail_local
    mid = (h + t) / 2
    d = t - h
    rot = d.to_track_quat("Z", "Y").to_euler()
    return part("cyl", bone, material, mid, (radius, radius, d.length / 2 + radius * 0.6), rot)


# body
part("cyl", "spine", TEAM, (0, 0, 1.36), (0.29, 0.22, 0.24))
part("cyl", "spine", VEST, (0, 0, 1.22), (0.31, 0.24, 0.12))
part("sphere", "spine", TEAM, (0.31, 0, 1.5), (0.12, 0.12, 0.12))
part("sphere", "spine", TEAM, (-0.31, 0, 1.5), (0.12, 0.12, 0.12))
part("cyl", "hips", TROUSERS, (0, 0, 1.03), (0.26, 0.2, 0.12))
# head: big, as the game's settlers have
part("sphere", "head", SKIN, (0, -0.02, 1.84), (0.25, 0.24, 0.27))
part("sphere", "head", HAIR, (0, 0.04, 1.93), (0.27, 0.26, 0.22))
part("sphere", "head", SKIN, (0, -0.27, 1.82), (0.06, 0.07, 0.07))
# arms
for s in ("L", "R"):
    limb(f"upperarm.{s}", TEAM, 0.085)
    limb(f"forearm.{s}", SKIN, 0.07)
    t = arm_data.bones[f"forearm.{s}"].tail_local
    part("sphere", f"forearm.{s}", SKIN, t, (0.08, 0.08, 0.08))
    limb(f"thigh.{s}", TROUSERS, 0.1)
    limb(f"shin.{s}", TROUSERS, 0.085)
    t = arm_data.bones[f"shin.{s}"].tail_local
    part("cube", f"shin.{s}", BOOTS, (t.x, t.y - 0.06, 0.06), (0.09, 0.15, 0.07))
# axe in the right hand, handle forward and up
hand = arm_data.bones["forearm.R"].tail_local
part("cyl", "forearm.R", WOOD, (hand.x, hand.y - 0.3, hand.z + 0.08), (0.03, 0.03, 0.42), (math.radians(-78), 0, 0))
part("cube", "forearm.R", STEEL, (hand.x, hand.y - 0.68, hand.z + 0.16), (0.02, 0.07, 0.11), (math.radians(-78), 0, 0))

# --- actions --------------------------------------------------------------------------------------
pose = rig.pose.bones
for pb in pose:
    pb.rotation_mode = "XYZ"


def key(frame, rots, root_z=0.0):
    for pb in pose:
        pb.rotation_euler = rots.get(pb.name, (0, 0, 0))
        pb.keyframe_insert("rotation_euler", frame=frame)
    pose["root"].location = (0, 0, root_z)
    pose["root"].keyframe_insert("location", frame=frame)


def new_action(name):
    rig.animation_data_create()
    rig.animation_data.action = None
    yield
    act = rig.animation_data.action
    act.name = name
    act.use_fake_user = True


# Bone X rotation on these down-pointing bones swings them in the YZ plane: + is forward (-Y) for
# legs and arms here, checked in the renders.
PERIOD = 24


def walk():
    steps = 8
    for i in range(steps + 1):
        ph = 2 * math.pi * i / steps
        sw = math.sin(ph)
        kneeL = 0.15 + 0.55 * max(0.0, math.sin(ph + math.pi / 2))
        kneeR = 0.15 + 0.55 * max(0.0, math.sin(ph + 3 * math.pi / 2))
        key(PERIOD * i / steps, {
            "thigh.L": (-0.5 * sw, 0, 0), "thigh.R": (0.5 * sw, 0, 0),
            "shin.L": (kneeL, 0, 0), "shin.R": (kneeR, 0, 0),
            "upperarm.L": (0.4 * sw, 0, 0), "upperarm.R": (-0.25 * sw - 0.2, 0, 0),
            "forearm.L": (-0.3, 0, 0), "forearm.R": (-0.7, 0, 0),
            "spine": (0.06, 0, 0.08 * sw),
        }, root_z=0.04 * math.cos(2 * ph))


def chop():
    # raise (0..9), hold, swing down to the hit at the middle (12 of 24, the axe sound's frame 6
    # of 12), recover.
    poses = [
        (0, -0.3, -0.6, 0.0),
        (6, -2.2, -0.5, -0.15),
        (9, -2.6, -0.4, -0.2),
        (12, -0.5, -0.1, 0.35),
        (15, -0.6, -0.3, 0.3),
        (24, -0.3, -0.6, 0.0),
    ]
    for f, up, fore, bend in poses:
        key(f, {
            "upperarm.L": (-up, 0.3, 0), "upperarm.R": (-up, -0.3, 0),
            "forearm.L": (fore, 0, 0), "forearm.R": (fore, 0, 0),
            "spine": (bend, 0, 0), "thigh.L": (0.25, 0, 0), "thigh.R": (-0.2, 0, 0),
            "shin.L": (0.2, 0, 0), "shin.R": (0.2, 0, 0),
        }, root_z=-0.04 * max(0.0, bend))


for name, fn in (("WC_WALK", walk), ("WC_CUT_TREE", chop)):
    g = new_action(name)
    next(g)
    fn()
    try:
        next(g)
    except StopIteration:
        pass

rig.animation_data.action = bpy.data.actions["WC_WALK"]
bpy.ops.wm.save_as_mainfile(filepath=OUT)
print(f"saved {OUT}: actions {[a.name for a in bpy.data.actions]}")
