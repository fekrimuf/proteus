"""Realism Probe (PRO_QUALITY_PLAN B12.1): render the same camera four ways in Blender Cycles.

  a  our procedural rock, flat grey, plain studio light
  b  our procedural rock with a real CC0 PBR rock material (box/triplanar projection, no UVs needed) under a real HDRI
  c  the scanned rock with its own CC0 material under the same HDRI
  d  the scanned rock, flat grey, plain studio light

Run (Blender is only ever started as a separate program):
  blender -b --factory-startup -P probe_render.py -- ours=our_rock.glb scan=boulder_01_1k.gltf \
      mat=<dir with rock_boulder_cracked_{diff,rough,disp}_1k.jpg> hdri=bloem_hill_01_1k.hdr out=<dir>

Both rocks are scaled to the same height and stand on the same plain ground; only shape and appearance differ.
Output: a.png b.png c.png d.png and probe.png (2x2: columns = flat | real appearance, rows = our shape | scanned shape).
"""
import math
import os
import sys

import bpy
import mathutils

args = dict(a.split("=", 1) for a in sys.argv[sys.argv.index("--") + 1:])
OUT = args["out"]
os.makedirs(OUT, exist_ok=True)
W, H, SAMPLES = 960, 720, int(args.get("samples", "96"))


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = SAMPLES
    sc.cycles.seed = 0  # same image every run
    sc.cycles.use_denoising = True
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = W, H, 100
    sc.view_settings.view_transform = "AgX"
    sc.render.image_settings.file_format = "PNG"
    return sc


def import_model(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    objs = [o for o in bpy.data.objects if o not in before and o.type == "MESH"]
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    obj.parent = None
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    # scale to a height of 1 m, stand on z = 0, centre in x/y
    corners = [obj.matrix_world @ mathutils.Vector(c) for c in obj.bound_box]
    lo = mathutils.Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
    hi = mathutils.Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
    s = 1.0 / (hi.z - lo.z)
    obj.scale = (s, s, s)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    corners = [obj.matrix_world @ mathutils.Vector(c) for c in obj.bound_box]
    lo = mathutils.Vector((min(c.x for c in corners), min(c.y for c in corners), min(c.z for c in corners)))
    hi = mathutils.Vector((max(c.x for c in corners), max(c.y for c in corners), max(c.z for c in corners)))
    obj.location = (-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z)
    bpy.ops.object.transform_apply(location=True, rotation=False, scale=False)
    return obj


def smooth(obj):
    for p in obj.data.polygons:
        p.use_smooth = True


def flat_material(name="flat_grey"):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.55, 0.55, 0.55, 1)
    b.inputs["Roughness"].default_value = 0.8
    return m


def pbr_box_material(d, scale):
    """Triplanar (box-projected) diffuse + roughness + bump from the displacement map: needs no UVs."""
    m = bpy.data.materials.new("rock_pbr_box")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    coord = nt.nodes.new("ShaderNodeTexCoord")
    mapping = nt.nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (scale, scale, scale)
    nt.links.new(coord.outputs["Object"], mapping.inputs["Vector"])

    def tex(name, colorspace):
        n = nt.nodes.new("ShaderNodeTexImage")
        n.image = bpy.data.images.load(os.path.join(d, name))
        n.image.colorspace_settings.name = colorspace
        n.projection = "BOX"
        n.projection_blend = 0.25
        nt.links.new(mapping.outputs["Vector"], n.inputs["Vector"])
        return n

    nt.links.new(tex("rock_boulder_cracked_diff_1k.jpg", "sRGB").outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(tex("rock_boulder_cracked_rough_1k.jpg", "Non-Color").outputs["Color"], bsdf.inputs["Roughness"])
    bump = nt.nodes.new("ShaderNodeBump")
    bump.inputs["Strength"].default_value = 0.8
    bump.inputs["Distance"].default_value = 0.02
    nt.links.new(tex("rock_boulder_cracked_disp_1k.jpg", "Non-Color").outputs["Color"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])
    return m


def ground():
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 0, 0))
    g = bpy.context.active_object
    g.data.materials.append(flat_material("ground"))
    g.data.materials[0].node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.42, 0.42, 0.42, 1)


def studio_light(sc):
    w = bpy.data.worlds.new("studio")
    w.use_nodes = True
    bg = w.node_tree.nodes["Background"]
    bg.inputs["Color"].default_value = (0.35, 0.36, 0.38, 1)
    bg.inputs["Strength"].default_value = 1.0
    sc.world = w
    bpy.ops.object.light_add(type="SUN", location=(3, -3, 5))
    sun = bpy.context.active_object
    sun.rotation_euler = (math.radians(48), math.radians(8), math.radians(35))
    sun.data.energy = 3.0
    sun.data.angle = math.radians(2.0)


def hdri_light(sc, path):
    w = bpy.data.worlds.new("hdri")
    w.use_nodes = True
    nt = w.node_tree
    env = nt.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(path)
    nt.links.new(env.outputs["Color"], nt.nodes["Background"].inputs["Color"])
    nt.nodes["Background"].inputs["Strength"].default_value = 1.0
    sc.world = w


def camera(sc):
    bpy.ops.object.camera_add(location=(2.7, -2.9, 1.25))
    cam = bpy.context.active_object
    cam.data.lens = 50
    target = mathutils.Vector((0, 0, 0.5))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
    sc.camera = cam


def render(name, model, material, lighting):
    sc = reset()
    obj = import_model(model)
    if material == "own":
        pass
    else:
        obj.data.materials.clear()
        obj.data.materials.append(material() if callable(material) else material)
    if material != "own" or "ours" in name:
        smooth(obj)
    ground()
    (studio_light(sc) if lighting == "studio" else hdri_light(sc, args["hdri"]))
    camera(sc)
    path = os.path.join(OUT, name + ".png")
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)
    return path


def main():
    mat_dir = args["mat"]
    paths = {}
    paths["a"] = render("a_ours_flat", args["ours"], flat_material, "studio")
    paths["b"] = render("b_ours_real_material", args["ours"], lambda: pbr_box_material(mat_dir, 0.72), "hdri")
    paths["c"] = render("c_scan_real_material", args["scan"], "own", "hdri")
    paths["d"] = render("d_scan_flat", args["scan"], flat_material, "studio")
    sc = reset()
    imgs = {k: bpy.data.images.load(v) for k, v in paths.items()}
    px = {k: list(i.pixels) for k, i in imgs.items()}
    out = bpy.data.images.new("probe", width=W * 2, height=H * 2, alpha=False)
    buf = [0.0] * (W * 2 * H * 2 * 4)
    # image rows are bottom-up: the top row of the sheet is the upper half of the buffer
    cells = {"a": (0, 1), "b": (1, 1), "d": (0, 0), "c": (1, 0)}  # (column, row-from-bottom)
    for k, (cx, cy) in cells.items():
        src = px[k]
        for y in range(H):
            s0 = y * W * 4
            d0 = ((cy * H + y) * W * 2 + cx * W) * 4
            buf[d0:d0 + W * 4] = src[s0:s0 + W * 4]
    out.pixels = buf
    out.filepath_raw = os.path.join(OUT, "probe.png")
    out.file_format = "PNG"
    out.save()
    print("PROBE_DONE", out.filepath_raw)


main()
