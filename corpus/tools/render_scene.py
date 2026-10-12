"""Render a Proteus glTF/GLB with Blender Cycles from a scene description (PRO_QUALITY_PLAN B12.2 steps 1 and 2).

Blender is only ever started as a separate program:
  blender -b --factory-startup -P render_scene.py -- model=world.glb scene=scene.json out=frame.png \
      library=corpus/materials/library.json corpus=D:/Fekri/corpus [report=report.json] [quick=1]

The glTF names materials (set_material in a recipe); this script binds the matching CC0 PBR set with box (triplanar)
projection, so nothing needs UVs. The scene file says everything else (sky, camera, render quality), see
docs/AGENT_GUIDE.md "Rendering a scene". Same files in, same image out (fixed Cycles seed).

Coordinates in the scene file are Proteus coordinates (Y up); glTF import turns them into Blender's Z up as (x, -z, y).
Sun azimuth is the heading of the direction TOWARD the sun in the XZ plane, measured from +X toward +Z, in degrees.
"""
import json
import math
import os
import re
import sys
import time

import bpy
import mathutils

args = dict(a.split("=", 1) for a in sys.argv[sys.argv.index("--") + 1:])
SCENE = json.load(open(args["scene"], encoding="utf-8"))
LIB = json.load(open(args["library"], encoding="utf-8"))["materials"] if os.path.exists(args.get("library", "")) else {}
CORPUS = args.get("corpus", "")
QUICK = args.get("quick", "0") == "1"
REPORT = {"blender": bpy.app.version_string, "warnings": []}


def warn(msg):
    REPORT["warnings"].append(msg)
    print("WARN", msg)


def to_b(v):
    """Proteus (x, y, z), Y up -> Blender (x, -z, y), Z up."""
    return mathutils.Vector((v[0], -v[2], v[1]))


# ---------------------------------------------------------------- render settings

def setup_render(sc):
    r = SCENE.get("render", {})
    sc.render.engine = "CYCLES"
    dev = r.get("device", "auto")
    use_gpu = False
    if dev in ("auto", "gpu"):
        try:
            prefs = bpy.context.preferences.addons["cycles"].preferences
            for t in ("OPTIX", "CUDA"):
                prefs.compute_device_type = t
                prefs.get_devices()
                gpus = [d for d in prefs.devices if d.type == t]
                if gpus:
                    for d in prefs.devices:
                        d.use = d.type == t
                    use_gpu = True
                    REPORT["device"] = t
                    break
        except Exception as e:  # no cycles prefs in odd builds
            warn("GPU setup failed: %s" % e)
    sc.cycles.device = "GPU" if use_gpu else "CPU"
    if not use_gpu:
        REPORT["device"] = "CPU"
    samples = int(r.get("samples", 64))
    w, h = r.get("resolution", [1280, 720])
    if QUICK:
        samples, w, h = max(8, samples // 4), w // 2, h // 2
    sc.cycles.samples = samples
    sc.cycles.use_adaptive_sampling = True
    sc.cycles.adaptive_threshold = float(r.get("noise_threshold", 0.02))
    sc.cycles.seed = int(r.get("seed", 0))
    sc.cycles.use_animated_seed = False
    sc.cycles.use_denoising = bool(r.get("denoise", True))
    sc.cycles.denoiser = "OPENIMAGEDENOISE"
    sc.cycles.sample_clamp_indirect = float(r.get("clamp_indirect", 10.0))
    sc.cycles.max_bounces = int(r.get("bounces", 6))
    # A ray through a forest crosses many alpha-cut leaf cards, and a shadow ray to the sun crosses every haze slab: both count as
    # transparent hits, and a ray that runs out of them turns black.
    sc.cycles.transparent_max_bounces = int(r.get("transparent_bounces", 64))
    sc.cycles.volume_bounces = int(r.get("volume_bounces", 1))
    wb = r.get("white_balance")
    if wb:
        try:
            sc.view_settings.use_white_balance = True
            sc.view_settings.white_balance_temperature = float(wb[0])
            sc.view_settings.white_balance_tint = float(wb[1]) if len(wb) > 1 else 10.0
        except AttributeError:
            warn("this Blender has no white balance setting; render.white_balance ignored")
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = int(w), int(h), 100
    sc.view_settings.view_transform = r.get("view_transform", "AgX")
    try:
        sc.view_settings.look = r.get("look", "AgX - Medium High Contrast")
    except TypeError:
        warn("unknown look '%s'; available: %s" % (r.get("look"), [i.identifier for i in sc.view_settings.bl_rna.properties["look"].enum_items]))
    # `exposure` is in stops around a calibrated setting: with a Nishita sky, 0 puts an 18% grey card in full sun at
    # 45 degrees near mid grey (measured: Blender's sun at intensity 1 needs about -5 stops; the base is -4, one stop hotter, so 25 degrees reads 0.54 and 45 degrees 0.66).
    base = -4.0 if SCENE.get("lighting", {}).get("sky", "nishita") == "nishita" else 0.0
    sc.view_settings.exposure = base + float(r.get("exposure", 0.0))
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_depth = "8"
    REPORT["samples"], REPORT["resolution"] = samples, [int(w), int(h)]


# ---------------------------------------------------------------- sky and light

def setup_world(sc):
    L = SCENE.get("lighting", {})
    w = bpy.data.worlds.new("world")
    w.use_nodes = True
    nt = w.node_tree
    bg = nt.nodes["Background"]
    out = nt.nodes["World Output"]
    sc.world = w
    kind = L.get("sky", "nishita")
    strength = float(L.get("strength", 1.0))
    if kind == "nishita":
        sky = nt.nodes.new("ShaderNodeTexSky")
        sky.sky_type = "NISHITA"
        sky.sun_elevation = math.radians(float(L.get("sun_elevation", 35.0)))
        sky.sun_rotation = math.radians(90.0 + float(L.get("sun_azimuth", 140.0)))  # calibrated against a rendered sky
        sky.sun_size = math.radians(float(L.get("sun_size", 0.545)))
        sky.sun_intensity = float(L.get("sun_intensity", 1.0))
        sky.air_density = float(L.get("air", 1.0))
        sky.dust_density = float(L.get("dust", 1.0))
        sky.ozone_density = float(L.get("ozone", 1.0))
        sky.altitude = float(L.get("altitude", 0.0))
        nt.links.new(sky.outputs["Color"], bg.inputs["Color"])
    elif kind == "hdri":
        path = L["hdri"]
        if not os.path.isabs(path):
            path = os.path.join(CORPUS, path)
        env = nt.nodes.new("ShaderNodeTexEnvironment")
        env.image = bpy.data.images.load(path, check_existing=True)
        mp = nt.nodes.new("ShaderNodeMapping")
        mp.inputs["Rotation"].default_value[2] = math.radians(float(L.get("rotation", 0.0)))
        tc = nt.nodes.new("ShaderNodeTexCoord")
        nt.links.new(tc.outputs["Generated"], mp.inputs["Vector"])
        nt.links.new(mp.outputs["Vector"], env.inputs["Vector"])
        nt.links.new(env.outputs["Color"], bg.inputs["Color"])
    elif kind == "flat":
        c = L.get("color", [0.35, 0.36, 0.38])
        bg.inputs["Color"].default_value = (c[0], c[1], c[2], 1.0)
    else:
        warn("unknown sky '%s', using flat grey" % kind)
    bg.inputs["Strength"].default_value = strength
    REPORT["sky"] = kind


def fog_slabs(haze, haze_h, top, avoid=()):
    """Split an exponential haze into nested slabs of constant density. Returns [(height above the base, added density)]:
    slab k reaches from below the base up to its height, and the densities of all slabs that cover a point add up, so the sum at
    height z follows haze * exp(-z / haze_h) in steps (each step holds the true mean over its range). A camera that sits right
    at a slab's lid looks along it for kilometres and sees the step as a line, so lids keep clear of the heights in `avoid`."""
    def mean(a, b):
        return haze * haze_h * (math.exp(-a / haze_h) - math.exp(-b / haze_h)) / (b - a)

    edges = [0.0]
    z = max(1.5, haze_h / 3.0)
    while z < top and len(edges) < 24:
        for a in avoid:
            if a > 0.0 and abs(z - a) < 0.2 * a:
                z = a * 1.3
        edges.append(z)
        z *= 1.55
    edges.append(max(top, edges[-1] * 1.2))
    rho = [mean(a, b) for a, b in zip(edges, edges[1:])]
    return [(edges[k + 1], rho[k] - (rho[k + 1] if k + 1 < len(rho) else 0.0)) for k in range(len(rho))]


def volume_box(name, center, size, build):
    """A box that holds only a volume shader; `build(node_tree)` returns the volume socket."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=center)
    c = bpy.context.active_object
    c.name = name
    c.scale = size
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    t = m.node_tree
    for n in list(t.nodes):
        if n.type == "BSDF_PRINCIPLED":
            t.nodes.remove(n)
    t.links.new(build(t), t.nodes["Material Output"].inputs["Volume"])
    c.data.materials.append(m)
    c.visible_shadow = False  # the air does not shade the ground; trees and ridges still cast their shafts into it
    return c, m


def add_fog(objs, cams):
    """Atmosphere in two parts, both falling off exponentially with height above `fog_base` (default: the lowest point of the model).

    HAZE (`fog_density`, `fog_height`): thin, kilometres deep, what turns far ridges pale and blue. Built from nested boxes of
    CONSTANT density, which Cycles integrates exactly; one box with a height-dependent density would be marched in steps of a
    tenth of the box (tens of kilometres here), which is both wrong and noisy.
    MIST (`mist_density`, `mist_height`): dense, tens of metres deep, lying on the valley floor around the cameras; this is what
    separates one line of trees from the next and carries the light shafts. It changes too fast with height for slabs (their
    lids show as lines), so it is one box with the true density, marched in steps of `mist_step` metres.

    A WORLD volume would be infinite and absorb every ray to the sun; boxes keep the sky clear."""
    L = SCENE.get("lighting", {})
    haze, mist = float(L.get("fog_density", 0.0)), float(L.get("mist_density", 0.0))
    if haze <= 0.0 and mist <= 0.0:
        return
    lo, hi = world_bounds(objs)
    haze_h, mist_h = max(1.0, float(L.get("fog_height", 300.0))), max(1.0, float(L.get("mist_height", 30.0)))
    extent = float(L.get("fog_extent", 6000.0))
    base = float(L["fog_base"]) if "fog_base" in L else lo.z
    col = L.get("fog_color", [0.8, 0.85, 0.95])
    aniso = float(L.get("fog_anisotropy", 0.3))
    sc = bpy.context.scene
    REPORT["fog"] = {"base": round(base, 2)}

    def scatter(t, density):
        vs = t.nodes.new("ShaderNodeVolumeScatter")
        if isinstance(density, float):
            vs.inputs["Density"].default_value = density
        else:
            t.links.new(density, vs.inputs["Density"])
        vs.inputs["Anisotropy"].default_value = aniso
        vs.inputs["Color"].default_value = (col[0], col[1], col[2], 1.0)
        return vs.outputs["Volume"]

    if haze > 0.0:
        top = max(6.0 * haze_h, min(hi.z - base, 12.0 * haze_h))
        floor = min(lo.z, base) - 20.0
        slabs = [(h, d) for h, d in fog_slabs(haze, haze_h, top, [c.location.z - base for _n, c in cams]) if d > 0.0]
        for k, (h, d) in enumerate(slabs):
            # every box a little wider and deeper than the last, so no two faces coincide
            z0, z1 = floor - k, base + h
            e = extent * (1.0 + 0.002 * k)
            _c, m = volume_box("haze_%02d" % k, (0, 0, (z0 + z1) / 2), (e, e, z1 - z0), lambda t, d=d: scatter(t, d))
            try:
                m.cycles.homogeneous_volume = True
            except AttributeError:
                pass  # newer Cycles finds out by itself that the density is constant
        REPORT["fog"].update({"haze_slabs": len(slabs), "haze_at_base": round(sum(d for _h, d in slabs), 7)})
    if mist > 0.0:
        ext = float(L.get("mist_extent", 5000.0))
        cx = sum(c.location.x for _n, c in cams) / len(cams)
        cy = sum(c.location.y for _n, c in cams) / len(cams)
        z0, z1 = base - 3.0 * mist_h, base + 7.0 * mist_h
        step = max(0.5, float(L.get("mist_step", mist_h / 3.0)))

        def density(t):
            # mist * exp(-max(z - base, 0) / mist_height): full strength in any hollow under the base
            pos = t.nodes.new("ShaderNodeNewGeometry")
            sep = t.nodes.new("ShaderNodeSeparateXYZ")
            t.links.new(pos.outputs["Position"], sep.inputs["Vector"])
            up = math_node(t, "MAXIMUM", math_node(t, "SUBTRACT", sep.outputs["Z"], base), 0.0)
            return scatter(t, math_node(t, "MULTIPLY", math_node(t, "EXPONENT", math_node(t, "MULTIPLY", up, -1.0 / mist_h)), mist))

        _c, m = volume_box("mist", (cx, cy, (z0 + z1) / 2), (ext, ext, z1 - z0), density)
        # Cycles marches a procedural volume in steps of a tenth of its box (average side) times the material's step rate
        rate = step / (0.1 * (ext + ext + (z1 - z0)) / 3.0)
        try:
            m.cycles.volume_step_rate = max(0.001, rate)
            sc.cycles.volume_step_rate = 1.0
            sc.cycles.volume_max_steps = int(min(4096, max(1024, 1.5 * ext / step)))
        except AttributeError:
            warn("this Cycles has no volume step settings; the mist may be coarse")
        REPORT["fog"].update({"mist_step_m": round(step, 2), "mist_box_m": [round(ext), round(z1 - z0)]})


# ---------------------------------------------------------------- materials

def tex_path(pid, kind):
    return os.path.join(CORPUS, "polyhaven", "materials", pid, "%s_%s_1k.jpg" % (pid, kind))


def flat_material(name, color, roughness=0.8, metallic=0.0, use_vertex_color=False, spec=None):
    """One colour. `variation` (0..1) with `variation_m` (metres) in the spec lets the brightness wander over the surface, so a
    merged mesh of many small things (a distant forest) is not one flat tone; `specular` overrides the default sheen."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    b = nt.nodes["Principled BSDF"]
    spec = spec or {}
    var = float(spec.get("variation", 0.0))
    if use_vertex_color:
        vc = nt.nodes.new("ShaderNodeVertexColor")
        nt.links.new(vc.outputs["Color"], b.inputs["Base Color"])
    elif var > 0.0:
        pos = nt.nodes.new("ShaderNodeNewGeometry").outputs["Position"]
        vm = float(spec.get("variation_m", 40.0))
        n = math_node(nt, "ADD", math_node(nt, "MULTIPLY", noise_field(nt, pos, vm, detail=2.0), 0.6), math_node(nt, "MULTIPLY", noise_field(nt, pos, vm / 6.0, detail=1.0, offset=(5.0, 3.0, 8.0)), 0.4))
        rgb = nt.nodes.new("ShaderNodeRGB")
        rgb.outputs[0].default_value = (color[0], color[1], color[2], 1.0)
        sc = nt.nodes.new("ShaderNodeVectorMath")
        sc.operation = "SCALE"
        nt.links.new(rgb.outputs[0], sc.inputs[0])
        nt.links.new(math_node(nt, "MULTIPLY_ADD", n, 2.0 * var, 1.0 - var), sc.inputs["Scale"])
        nt.links.new(sc.outputs["Vector"], b.inputs["Base Color"])
    else:
        b.inputs["Base Color"].default_value = (color[0], color[1], color[2], 1.0)
    b.inputs["Roughness"].default_value = roughness
    b.inputs["Metallic"].default_value = metallic
    if "specular" in spec:
        b.inputs["Specular IOR Level"].default_value = float(spec["specular"])
    return m


def pbr_layer(nt, spec, src):
    """The texture lookups of one library material, built inside node tree `nt` and fed by the coordinate socket `src`.
    Returns (color, rough, metal, height) sockets; rough, metal and height are None when the set has no such map."""
    pid = spec["polyhaven"]
    info = nt.nodes.new("ShaderNodeObjectInfo")  # Random: one number per object, so instances differ
    jitter = nt.nodes.new("ShaderNodeCombineXYZ")
    for axis, k in zip(("X", "Y", "Z"), (13.1, 7.7, 3.3)):
        mul = nt.nodes.new("ShaderNodeMath")
        mul.operation = "MULTIPLY"
        mul.inputs[1].default_value = k
        nt.links.new(info.outputs["Random"], mul.inputs[0])
        nt.links.new(mul.outputs["Value"], jitter.inputs[axis])
    mapping = nt.nodes.new("ShaderNodeMapping")
    s = 1.0 / max(1e-3, float(spec.get("scale_m", 1.0)))
    mapping.inputs["Scale"].default_value = (s, s, s)
    nt.links.new(src, mapping.inputs["Vector"])
    nt.links.new(jitter.outputs["Vector"], mapping.inputs["Location"])

    # Anti-tiling: every map is sampled twice (second sample smaller, rotated and offset) and the two are blended by slow
    # world-scale noise, so the texture repeat that shows as stripes toward the horizon is broken up.
    antitile = bool(spec.get("antitile", True))
    if antitile:
        mapping_b = nt.nodes.new("ShaderNodeMapping")
        sb = s * 0.31
        mapping_b.inputs["Scale"].default_value = (sb, sb, sb)
        mapping_b.inputs["Rotation"].default_value = (0.0, 0.0, 0.65)
        offs = nt.nodes.new("ShaderNodeVectorMath")
        offs.operation = "ADD"
        offs.inputs[1].default_value = (7.3, 2.1, 5.9)
        nt.links.new(jitter.outputs["Vector"], offs.inputs[0])
        nt.links.new(src, mapping_b.inputs["Vector"])
        nt.links.new(offs.outputs["Vector"], mapping_b.inputs["Location"])
        noise = nt.nodes.new("ShaderNodeTexNoise")
        noise.inputs["Scale"].default_value = float(spec.get("antitile_scale", 0.06))
        noise.inputs["Detail"].default_value = 2.0
        nt.links.new(src, noise.inputs["Vector"])
        remap = nt.nodes.new("ShaderNodeMapRange")
        remap.inputs["From Min"].default_value = 0.35
        remap.inputs["From Max"].default_value = 0.65
        nt.links.new(noise.outputs["Fac"], remap.inputs["Value"])

    def tex_node(kind, colorspace, vector):
        p = tex_path(pid, kind)
        if not os.path.exists(p):
            return None
        n = nt.nodes.new("ShaderNodeTexImage")
        n.image = bpy.data.images.load(p, check_existing=True)
        n.image.colorspace_settings.name = colorspace
        n.projection = "BOX"
        n.projection_blend = 0.3
        nt.links.new(vector, n.inputs["Vector"])
        return n

    def tex(kind, colorspace):
        """Colour output socket of a map (blended anti-tiled pair), or None if the set has no such map."""
        a = tex_node(kind, colorspace, mapping.outputs["Vector"])
        if a is None or not antitile:
            return a.outputs["Color"] if a else None
        b = tex_node(kind, colorspace, mapping_b.outputs["Vector"])
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        nt.links.new(remap.outputs["Result"], mix.inputs[0])
        nt.links.new(a.outputs["Color"], mix.inputs[6])
        nt.links.new(b.outputs["Color"], mix.inputs[7])
        return mix.outputs[2]

    color_out = tex("diff", "sRGB")
    var = float(spec.get("variation", 0.08))
    sat = float(spec.get("saturation", 1.0))  # under 1 greys a scan whose hue does not suit the place (a red soil in a granite canyon)
    if sat != 1.0 and var <= 0:
        hsv = nt.nodes.new("ShaderNodeHueSaturation")
        hsv.inputs["Saturation"].default_value = sat
        nt.links.new(color_out, hsv.inputs["Color"])
        color_out = hsv.outputs["Color"]
    if var > 0:
        hsv = nt.nodes.new("ShaderNodeHueSaturation")
        hsv.inputs["Saturation"].default_value = sat
        sub = nt.nodes.new("ShaderNodeMath"); sub.operation = "SUBTRACT"; sub.inputs[1].default_value = 0.5
        mul = nt.nodes.new("ShaderNodeMath"); mul.operation = "MULTIPLY"; mul.inputs[1].default_value = 2.0 * var
        add = nt.nodes.new("ShaderNodeMath"); add.operation = "ADD"; add.inputs[1].default_value = 1.0
        nt.links.new(info.outputs["Random"], sub.inputs[0])
        nt.links.new(sub.outputs["Value"], mul.inputs[0])
        nt.links.new(mul.outputs["Value"], add.inputs[0])
        nt.links.new(add.outputs["Value"], hsv.inputs["Value"])
        nt.links.new(color_out, hsv.inputs["Color"])
        color_out = hsv.outputs["Color"]
    if spec.get("tint"):
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.inputs[0].default_value = 1.0
        t = spec["tint"]
        mix.inputs[7].default_value = (t[0], t[1], t[2], 1.0)
        nt.links.new(color_out, mix.inputs[6])
        color_out = mix.outputs[2]
    height = tex("disp", "Non-Color") if float(spec.get("bump", 0.5)) > 0 else None
    return color_out, tex("rough", "Non-Color"), tex("metal", "Non-Color"), height


def textured(spec):
    return bool(spec and "polyhaven" in spec and os.path.exists(tex_path(spec["polyhaven"], "diff")))


def finish_bsdf(nt, bsdf, color, rough, metal, height, spec):
    nt.links.new(color, bsdf.inputs["Base Color"])
    if rough is not None:
        nt.links.new(rough, bsdf.inputs["Roughness"])
    if metal is not None:
        nt.links.new(metal, bsdf.inputs["Metallic"])
    if height is not None:
        bump = nt.nodes.new("ShaderNodeBump")
        bump.inputs["Strength"].default_value = float(spec.get("bump", 0.5))
        bump.inputs["Distance"].default_value = float(spec.get("bump_distance", 0.02))
        nt.links.new(height, bump.inputs["Height"])
        nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])


def coordinate_source(nt, spec):
    if spec.get("space", "object") == "world":
        return nt.nodes.new("ShaderNodeNewGeometry").outputs["Position"]
    return nt.nodes.new("ShaderNodeTexCoord").outputs["Object"]


def pbr_material(name, spec):
    """Triplanar PBR from a Poly Haven set. Returns (material, resolved_with_textures)."""
    if not textured(spec):
        warn("material '%s': files for %s are not on disk (pt corpus fetch corpus/materials_manifest.json); flat colour used" % (name, spec.get("polyhaven")))
        return flat_material(name, spec.get("fallback", [0.5, 0.5, 0.5]), spec.get("roughness", 0.8), spec.get("metallic", 0.0)), False
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    color, rough, metal, height = pbr_layer(nt, spec, coordinate_source(nt, spec))
    finish_bsdf(nt, bsdf, color, rough, metal, height, spec)
    return m, True


def first_channel(nt, sock):
    """Poly Haven single-channel maps are stored as RGB; after weighting, read the red channel back as a number."""
    if sock is None:
        return None
    sp = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(sock, sp.inputs["Color"])
    return sp.outputs["Red"]


def math_node(nt, op, a, b=None, c=None, clamp=False):
    """One Math node; each of a, b, c is a socket or a number. Returns its output socket."""
    n = nt.nodes.new("ShaderNodeMath")
    n.operation = op
    n.use_clamp = clamp
    for i, v in enumerate((a, b, c)):
        if v is None:
            continue
        if isinstance(v, (int, float)):
            n.inputs[i].default_value = float(v)
        else:
            nt.links.new(v, n.inputs[i])
    return n.outputs["Value"]


def noise_field(nt, src, metres, detail=2.0, offset=(0.0, 0.0, 0.0), kind="FBM", roughness=0.5, stretch=(1.0, 1.0, 1.0), lo=0.3, hi=0.7):
    """A noise value spread over 0..1, with features about `metres` across in the coordinate `src` (which is in metres).
    `stretch` scales the axes (Blender axes: Z is up), so (0.2, 0.2, 3) gives level bands like rock strata."""
    mp = nt.nodes.new("ShaderNodeMapping")
    s = 1.0 / max(1e-4, metres)
    mp.inputs["Scale"].default_value = (s * stretch[0], s * stretch[1], s * stretch[2])
    mp.inputs["Location"].default_value = offset
    nt.links.new(src, mp.inputs["Vector"])
    n = nt.nodes.new("ShaderNodeTexNoise")
    n.inputs["Scale"].default_value = 1.0
    n.inputs["Detail"].default_value = detail
    n.inputs["Roughness"].default_value = roughness
    try:
        n.noise_type = kind
        n.normalize = True
    except (AttributeError, TypeError):
        pass  # older Blender: plain fBm only
    nt.links.new(mp.outputs["Vector"], n.inputs["Vector"])
    r = nt.nodes.new("ShaderNodeMapRange")
    r.inputs["From Min"].default_value, r.inputs["From Max"].default_value = lo, hi
    nt.links.new(n.outputs["Fac"], r.inputs["Value"])
    return r.outputs["Result"]


def layered_material(name, role_specs, mspec=None):
    """One material that mixes several library materials by per-vertex weights (`_WEIGHTS_0`, `_WEIGHTS_1`, written by
    blend_materials). Colour, roughness, metal and height are blended with the weights; one Principled BSDF shades the result.

    Vertex weights are as coarse as the mesh (25 m cells on a far ring), so on their own they give soft watercolour blends and
    smooth, plaster-like slopes. Detail finer than the mesh is made here, per pixel, and each piece is switched on per ROLE:
      breakup, breakup_m   push the weight through a noise threshold, so a soft gradient becomes a crisp patchwork (snow patches,
                           tongues of grass in scree); breakup_by="relief" uses the rock relief, so snow lies in the gullies
      macro                slow brightness change over `macro_m` metres (material key), so a slope is not one flat colour
      relief               metres of ridged rock relief added to the bump height, `relief_m` across (material key)
      strata               level banding in colour, like bedded rock
    Heights are in metres (map value x bump_distance x bump), so a 90 m aerial scan and a 2 m close-up scan bump correctly side by side."""
    mspec = mspec or {}
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    wsock = []  # weight socket per layer
    for att, count in (("_WEIGHTS_0", min(4, len(role_specs))), ("_WEIGHTS_1", len(role_specs) - 4)):
        if count <= 0:
            continue
        a = nt.nodes.new("ShaderNodeAttribute")
        a.attribute_type = "GEOMETRY"
        a.attribute_name = att
        sep = nt.nodes.new("ShaderNodeSeparateColor")
        nt.links.new(a.outputs["Color"], sep.inputs["Color"])
        socks = [sep.outputs["Red"], sep.outputs["Green"], sep.outputs["Blue"], a.outputs["Alpha"]]
        wsock.extend(socks[:count])

    # shared fields of the material (built only when a role asks for them)
    world = nt.nodes.new("ShaderNodeTexCoord").outputs["Object"]
    fields = {}

    def field(kind, metres=None):
        """A slow field over the surface, 0..1; one node group per (kind, size), shared by the roles that ask for it."""
        metres = float(metres if metres is not None else mspec.get(kind + "_m", {"macro": 160.0, "relief": 60.0, "strata": 14.0}[kind]))
        key = (kind, metres)
        if key not in fields:
            if kind == "macro":
                a = noise_field(nt, world, metres, detail=3.0, offset=(11.0, 5.0, 2.0))
                b = noise_field(nt, world, metres / 7.0, detail=2.0, offset=(3.0, 17.0, 9.0))
                fields[key] = math_node(nt, "ADD", math_node(nt, "MULTIPLY", a, 0.65), math_node(nt, "MULTIPLY", b, 0.35))
            elif kind == "relief":
                a = noise_field(nt, world, metres, detail=5.0, roughness=0.55, kind="RIDGED_MULTIFRACTAL", offset=(4.0, 8.0, 1.0), lo=0.15, hi=0.85)
                b = noise_field(nt, world, metres / 9.0, detail=3.0, offset=(9.0, 2.0, 6.0))
                fields[key] = math_node(nt, "ADD", math_node(nt, "MULTIPLY", a, 0.75), math_node(nt, "MULTIPLY", b, 0.25))
            else:
                fields[key] = noise_field(nt, world, metres, detail=3.0, stretch=(0.12, 0.12, 1.0), offset=(1.0, 6.0, 3.0))
        return fields[key]

    # crisp patchwork: each weight goes through a noise threshold, then the weights are brought back to a sum of one
    if any(float(s.get("breakup", 0.0)) > 0.0 for s in role_specs):
        mixed = []
        for i, spec in enumerate(role_specs):
            amount = float(spec.get("breakup", 0.0))
            if amount <= 0.0:
                mixed.append(wsock[i])
                continue
            soft = max(0.02, float(spec.get("breakup_soft", 0.18)))
            if spec.get("breakup_by") == "relief":
                n = field("relief", spec.get("breakup_m"))
            else:
                n = noise_field(nt, world, float(spec.get("breakup_m", 30.0)), detail=3.0, offset=(17.0 * i + 3.0, 5.0 * i + 1.0, 29.0 * i))
            # threshold kept inside [soft/2, 1 - soft/2], so a weight of 0 stays 0 and a weight of 1 stays 1
            thr = nt.nodes.new("ShaderNodeMapRange")
            thr.inputs["To Min"].default_value, thr.inputs["To Max"].default_value = soft / 2.0, 1.0 - soft / 2.0
            nt.links.new(n, thr.inputs["Value"])
            cut = math_node(nt, "MULTIPLY_ADD", math_node(nt, "SUBTRACT", wsock[i], thr.outputs["Result"]), 1.0 / soft, 0.5, clamp=True)
            mixed.append(math_node(nt, "ADD", math_node(nt, "MULTIPLY", wsock[i], 1.0 - amount), math_node(nt, "MULTIPLY", cut, amount)))
        total = None
        kept = []
        for i, w in enumerate(mixed):
            w = math_node(nt, "ADD", w, math_node(nt, "MULTIPLY", wsock[i], 0.02))  # where every layer was cut away, the soft blend shows
            kept.append(w)
            total = w if total is None else math_node(nt, "ADD", total, w)
        wsock = [math_node(nt, "DIVIDE", w, math_node(nt, "MAXIMUM", total, 1e-4)) for w in kept]
    acc = {"color": None, "rough": None, "metal": None, "height": None, "spec": None}

    def accumulate(key, sock, w, vector):
        if sock is None:
            return
        n = nt.nodes.new("ShaderNodeVectorMath" if vector else "ShaderNodeMath")
        if vector:
            n.operation = "SCALE"
            nt.links.new(sock, n.inputs[0])
            nt.links.new(w, n.inputs["Scale"])
            term = n.outputs["Vector"]
        else:
            n.operation = "MULTIPLY"
            nt.links.new(sock, n.inputs[0])
            nt.links.new(w, n.inputs[1])
            term = n.outputs["Value"]
        if acc[key] is None:
            acc[key] = term
        else:
            add = nt.nodes.new("ShaderNodeVectorMath" if vector else "ShaderNodeMath")
            add.operation = "ADD"
            nt.links.new(acc[key], add.inputs[0])
            nt.links.new(term, add.inputs[1])
            acc[key] = add.outputs["Vector" if vector else "Value"]

    for i, spec in enumerate(role_specs):
        if textured(spec):
            color, rough, metal, height = pbr_layer(nt, spec, coordinate_source(nt, spec))
            rough = first_channel(nt, rough)
            metal = first_channel(nt, metal)
            height = first_channel(nt, height)
            if height is not None:  # map value 0..1 -> metres
                height = math_node(nt, "MULTIPLY", height, float(spec.get("bump_distance", 0.02)) * float(spec.get("bump", 0.5)))
        else:
            if spec.get("polyhaven"):
                warn("layer files for %s are not on disk; flat colour used" % spec["polyhaven"])
            c = spec.get("fallback", spec.get("color", [0.5, 0.5, 0.5]))
            rgb = nt.nodes.new("ShaderNodeRGB")
            rgb.outputs[0].default_value = (c[0], c[1], c[2], 1.0)
            color, rough, metal, height = rgb.outputs[0], None, None, None
        gain = None  # brightness factor from the slow fields
        for key, kind in (("macro", "macro"), ("strata", "strata")):
            amp = float(spec.get(key, 0.0))
            if amp > 0.0:
                g = math_node(nt, "MULTIPLY_ADD", field(kind), 2.0 * amp, 1.0 - amp)  # 1 - amp .. 1 + amp
                gain = g if gain is None else math_node(nt, "MULTIPLY", gain, g)
        relief = float(spec.get("relief", 0.0))
        if relief > 0.0:
            rf = field("relief", spec.get("relief_m"))
            r = math_node(nt, "MULTIPLY", rf, relief)
            height = r if height is None else math_node(nt, "ADD", height, r)
            # ribs catch the light and gullies hold shade and dirt: a little of the relief goes into the colour too
            shade = float(spec.get("relief_shade", 0.35))
            g = math_node(nt, "MULTIPLY_ADD", rf, shade, 1.0 - 0.5 * shade)
            gain = g if gain is None else math_node(nt, "MULTIPLY", gain, g)
        if gain is not None:
            sc = nt.nodes.new("ShaderNodeVectorMath")
            sc.operation = "SCALE"
            nt.links.new(color, sc.inputs[0])
            nt.links.new(gain, sc.inputs["Scale"])
            color = sc.outputs["Vector"]
        if rough is None:
            rn = nt.nodes.new("ShaderNodeValue")
            rn.outputs[0].default_value = float(spec.get("roughness", 0.85))
            rough = rn.outputs[0]
        # Ground and canopy are matte: a dark surface seen at a grazing angle would otherwise reflect enough bright sky to turn teal at
        # a distance. Only a glossy layer (water: roughness under 0.2) keeps a strong specular; a role may set "specular" itself.
        sv = nt.nodes.new("ShaderNodeValue")
        sv.outputs[0].default_value = float(spec.get("specular", 0.6 if float(spec.get("roughness", 0.85)) < 0.2 else 0.08))
        accumulate("spec", sv.outputs[0], wsock[i], False)
        accumulate("color", color, wsock[i], True)
        accumulate("rough", rough, wsock[i], False)
        accumulate("metal", metal, wsock[i], False)
        accumulate("height", height, wsock[i], False)
    # heights are already in metres, so the bump node works at a distance of one
    finish_bsdf(nt, bsdf, acc["color"], acc["rough"], acc["metal"], acc["height"], {"bump": float(mspec.get("bump", 1.0)), "bump_distance": 1.0})
    if acc["spec"] is not None:
        nt.links.new(acc["spec"], bsdf.inputs["Specular IOR Level"])
    return m


def water_material(name, spec):
    """Clear water for a surface that lies over a bed: it refracts (you see the stones under it close by), mirrors the sky at a
    grazing angle (Fresnel), carries small ripples as a bump, and tints with depth. Cycles has no caustics here, so a refracting
    surface would leave its bed in black shadow; shadow rays therefore pass straight through, tinted."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tint = spec.get("tint", [0.78, 0.90, 0.86])
    bsdf.inputs["Base Color"].default_value = (tint[0], tint[1], tint[2], 1.0)
    bsdf.inputs["Roughness"].default_value = float(spec.get("roughness", 0.02))
    bsdf.inputs["IOR"].default_value = 1.333
    bsdf.inputs["Transmission Weight"].default_value = 1.0
    pos = nt.nodes.new("ShaderNodeNewGeometry").outputs["Position"]
    rm = max(0.02, float(spec.get("ripple_m", 0.9)))
    small = noise_field(nt, pos, rm, detail=3.0, offset=(2.0, 7.0, 0.0), lo=0.0, hi=1.0)
    swell = noise_field(nt, pos, rm * 6.0, detail=2.0, offset=(9.0, 1.0, 0.0), lo=0.0, hi=1.0)
    h = math_node(nt, "ADD", math_node(nt, "MULTIPLY", small, 0.02), math_node(nt, "MULTIPLY", swell, 0.06))
    bump = nt.nodes.new("ShaderNodeBump")
    bump.inputs["Strength"].default_value = float(spec.get("ripple", 0.25))
    bump.inputs["Distance"].default_value = 1.0
    nt.links.new(h, bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])
    out = nt.nodes["Material Output"]
    through = nt.nodes.new("ShaderNodeBsdfTransparent")
    through.inputs["Color"].default_value = (tint[0], tint[1], tint[2], 1.0)
    lp = nt.nodes.new("ShaderNodeLightPath")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(lp.outputs["Is Shadow Ray"], mix.inputs["Fac"])
    nt.links.new(bsdf.outputs["BSDF"], mix.inputs[1])
    nt.links.new(through.outputs["BSDF"], mix.inputs[2])
    nt.links.new(mix.outputs["Shader"], out.inputs["Surface"])
    depth = float(spec.get("absorb", 0.35))
    if depth > 0.0:
        ab = nt.nodes.new("ShaderNodeVolumeAbsorption")
        dc = spec.get("deep", [0.45, 0.72, 0.66])
        ab.inputs["Color"].default_value = (dc[0], dc[1], dc[2], 1.0)
        ab.inputs["Density"].default_value = depth
        nt.links.new(ab.outputs["Volume"], out.inputs["Volume"])
    return m


def material_spec(name):
    """Library entry with the scene file's per-material overrides merged on top; None if the name is unknown."""
    s = LIB.get(name)
    o = SCENE.get("materials", {}).get(name)
    if s is None and o is None:
        return None
    if o and o.get("use"):  # a ROLE name ("role_ground") that the scene maps onto a library material, plus its own tweaks
        base = LIB.get(o["use"])
        if base is None:
            warn("material '%s' says use='%s' but the library has no such material" % (name, o["use"]))
        s = base if base is not None else s
    merged = dict(s or {})
    merged.update({k: v for k, v in (o or {}).items() if k != "use"})
    return merged if "polyhaven" in merged or "fallback" in merged or "color" in merged or "layers" in merged else None


class Materials:
    def __init__(self):
        self.cache = {}
        self.layered = set()
        self.stats = {}  # name -> {"resolved": bool, "textured": bool, "triangles": int}
        d = SCENE.get("default_material", {})
        self.default_color = d.get("color", [0.55, 0.55, 0.55])
        self.default_rough = d.get("roughness", 0.8)

    def spec(self, name):
        return material_spec(name)

    def get(self, name, has_vertex_color):
        key = (name, has_vertex_color)
        if key in self.cache:
            return self.cache[key]
        if name is None:
            m = flat_material("unassigned_vc" if has_vertex_color else "unassigned", self.default_color, self.default_rough, 0.0, has_vertex_color)
            self.stats.setdefault("(unassigned)", {"resolved": False, "textured": False, "triangles": 0})
        else:
            spec = self.spec(name)
            if spec is None:
                warn("material '%s' is not in the library or the scene file; default colour used" % name)
                m = flat_material(name, self.default_color, self.default_rough)
                self.stats.setdefault(name, {"resolved": False, "textured": False, "triangles": 0})
            elif "layers" in spec:
                role_specs = []
                for role in spec["layers"]:
                    rs = material_spec(role)
                    if rs is None:
                        warn("layer '%s' of '%s' is not in the library or the scene file; grey used" % (role, name))
                        rs = {"color": self.default_color}
                    role_specs.append(rs)
                m = layered_material(name, role_specs, spec)
                self.layered.add(name)
                self.stats.setdefault(name, {"resolved": all(material_spec(r) is not None for r in spec["layers"]), "textured": all(textured(r) for r in role_specs), "layered": spec["layers"], "triangles": 0})
            elif spec.get("kind") == "water":
                m = water_material(name, spec)
                self.stats.setdefault(name, {"resolved": True, "textured": True, "kind": "water", "triangles": 0})
            elif "polyhaven" in spec:
                m, textured_ok = pbr_material(name, spec)
                self.stats.setdefault(name, {"resolved": True, "textured": textured_ok, "triangles": 0})
            else:
                c = spec.get("color", spec.get("fallback", self.default_color))
                m = flat_material(name, c, spec.get("roughness", 0.8), spec.get("metallic", 0.0), spec=spec)
                self.stats.setdefault(name, {"resolved": True, "textured": False, "triangles": 0})
        self.cache[key] = m
        return m


def has_image_texture(m):
    return bool(m.use_nodes and m.node_tree and any(n.type == "TEX_IMAGE" for n in m.node_tree.nodes))


def tune_authored(m, spec):
    """A scanned asset keeps its own material, but a style may still say `saturation` and `tint` for it (keyed by the material's
    name as the report lists it, `rock_moss_set_01__rock_moss_set_01`): a rock scanned on red soil in another country is too warm
    for a grey granite canyon, and the scan cannot know that. Returns True if the material was changed."""
    if not spec or not ("saturation" in spec or "tint" in spec) or any(n.name == "pt_tune" for n in m.node_tree.nodes):
        return False
    nt = m.node_tree
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None or not bsdf.inputs["Base Color"].is_linked:
        return False
    src = bsdf.inputs["Base Color"].links[0].from_socket
    hsv = nt.nodes.new("ShaderNodeHueSaturation")
    hsv.name = "pt_tune"
    hsv.inputs["Saturation"].default_value = float(spec.get("saturation", 1.0))
    nt.links.new(src, hsv.inputs["Color"])
    out = hsv.outputs["Color"]
    if spec.get("tint"):
        t = spec["tint"]
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.inputs[0].default_value = 1.0
        mix.inputs[7].default_value = (t[0], t[1], t[2], 1.0)
        nt.links.new(out, mix.inputs[6])
        out = mix.outputs[2]
    nt.links.new(out, bsdf.inputs["Base Color"])
    return True


def add_translucency(m, amount):
    """Leaves, needles and grass let light through; a scanned plant's material does not say so, and against the sun it renders as
    black card. For an authored material that is alpha-cut (its Alpha comes from a texture), part of the surface becomes a
    translucent layer in the leaf's own colour, cut by the same alpha. Returns True if the material was changed."""
    nt = m.node_tree
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    out = next((n for n in nt.nodes if n.type == "OUTPUT_MATERIAL" and n.is_active_output), None)
    if bsdf is None or out is None or not bsdf.inputs["Alpha"].is_linked or any(n.type == "BSDF_TRANSLUCENT" for n in nt.nodes):
        return False
    alpha = bsdf.inputs["Alpha"].links[0].from_socket
    tr = nt.nodes.new("ShaderNodeBsdfTranslucent")
    if bsdf.inputs["Base Color"].is_linked:
        nt.links.new(bsdf.inputs["Base Color"].links[0].from_socket, tr.inputs["Color"])
    else:
        tr.inputs["Color"].default_value = bsdf.inputs["Base Color"].default_value
    clear = nt.nodes.new("ShaderNodeBsdfTransparent")
    cut = nt.nodes.new("ShaderNodeMixShader")  # alpha 0 -> see through, alpha 1 -> translucent leaf
    nt.links.new(alpha, cut.inputs["Fac"])
    nt.links.new(clear.outputs["BSDF"], cut.inputs[1])
    nt.links.new(tr.outputs["BSDF"], cut.inputs[2])
    mix = nt.nodes.new("ShaderNodeMixShader")
    mix.inputs["Fac"].default_value = amount
    nt.links.new(bsdf.outputs["BSDF"], mix.inputs[1])
    nt.links.new(cut.outputs["Shader"], mix.inputs[2])
    nt.links.new(mix.outputs["Shader"], out.inputs["Surface"])
    return True


def base_name(n):
    return re.sub(r"\.\d{3}$", "", n)


def bind_materials(objs):
    mats = Materials()
    translucency = float(SCENE.get("render", {}).get("foliage_translucency", 0.0))
    tris_by_mesh = {}
    users = {}
    for o in objs:
        users[o.data.name] = users.get(o.data.name, 0) + 1
    done = set()
    for o in objs:
        me = o.data
        if me.name in done:
            continue
        done.add(me.name)
        has_vc = len(me.color_attributes) > 0
        if len(me.materials) == 0:
            me.materials.append(None)
        for i in range(len(me.materials)):
            cur = me.materials[i]
            name = base_name(cur.name) if cur is not None else None
            if cur is not None and has_image_texture(cur):
                # authored in the source asset (textures, alpha mode, double-sided as imported): keep it
                st = mats.stats.setdefault(name, {"resolved": True, "textured": True, "authored": True, "triangles": 0})
                if tune_authored(cur, SCENE.get("materials", {}).get(name)):
                    st["tuned"] = True
                if translucency > 0.0 and add_translucency(cur, translucency):
                    st["translucent"] = True
                continue
            me.materials[i] = mats.get(name, has_vc)
            if name in mats.layered and "_WEIGHTS_0" not in me.attributes:
                warn("layered material '%s' is on a mesh without the _WEIGHTS_0 attribute (found: %s); only its first layer shows" % (name, list(me.attributes.keys())))
        me.calc_loop_triangles()
        per_slot = {}
        for t in me.loop_triangles:
            per_slot[t.material_index] = per_slot.get(t.material_index, 0) + 1
        for i, n in per_slot.items():
            slot = me.materials[min(i, len(me.materials) - 1)]
            nm = base_name(slot.name)
            key = nm if nm in mats.stats else "(unassigned)"
            mats.stats[key]["triangles"] += n * users[me.name]
    total = sum(s["triangles"] for s in mats.stats.values())
    textured = sum(s["triangles"] for s in mats.stats.values() if s["textured"])
    REPORT["materials"] = mats.stats
    REPORT["triangles"] = total
    REPORT["material_coverage"] = round(textured / total, 4) if total else 0.0
    REPORT["unresolved"] = sorted(n for n, s in mats.stats.items() if not s["resolved"])
    return mats


# ---------------------------------------------------------------- scene assembly

def import_model(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    objs = [o for o in bpy.data.objects if o not in before and o.type == "MESH"]
    # LOD exports carry one node per level (`inst_12__lod2`): the picture uses level 0 only
    lower = [o for o in objs if re.search(r"__lod[1-9]\d*(\.\d+)?$", o.name)]
    for o in lower:
        bpy.data.objects.remove(o, do_unlink=True)
    REPORT["lod_nodes_dropped"] = len(lower)
    objs = [o for o in objs if o not in set(lower)]
    REPORT["objects"] = len(objs)
    REPORT["unique_meshes"] = len({o.data.name for o in objs})
    return objs


def smooth(objs, angle_deg):
    if angle_deg <= 0:
        return
    seen = set()
    bpy.ops.object.select_all(action="DESELECT")
    pick = []
    for o in objs:
        if o.data.name not in seen:
            seen.add(o.data.name)
            if not o.data.has_custom_normals:  # normals that came with the model (assets, `normals` nodes) stay as authored
                pick.append(o)
    if not pick:
        return
    for o in pick:
        o.select_set(True)
    bpy.context.view_layer.objects.active = pick[0]
    bpy.ops.object.shade_smooth_by_angle(angle=math.radians(angle_deg))


def world_bounds(objs):
    lo = mathutils.Vector((1e30, 1e30, 1e30))
    hi = mathutils.Vector((-1e30, -1e30, -1e30))
    for o in objs:
        mw = o.matrix_world
        for c in o.bound_box:
            v = mw @ mathutils.Vector(c)
            lo = mathutils.Vector((min(lo.x, v.x), min(lo.y, v.y), min(lo.z, v.z)))
            hi = mathutils.Vector((max(hi.x, v.x), max(hi.y, v.y), max(hi.z, v.z)))
    return lo, hi


def add_ground(sc, objs):
    """An endless plane under the model, so the view to the horizon is ground and not the dark lower half of the sky.
    On by default (material "dirt"); `"ground": false` turns it off."""
    g = SCENE.get("ground", {})
    if g is False:
        return
    g = g if isinstance(g, dict) else {}
    lo, _ = world_bounds(objs)
    size = float(g.get("size", 400000.0))
    y = g.get("height")
    z = lo.z - 0.02 if y is None else float(y)
    bpy.ops.mesh.primitive_plane_add(size=size, location=(0, 0, z))
    pl = bpy.context.active_object
    pl.name = "ground"
    name = g.get("material", "dirt")
    spec = material_spec(name)
    if spec and "polyhaven" in spec:
        s = dict(spec)
        s["space"] = "world"
        m, _ = pbr_material(name, s)
    else:
        m = flat_material(name, (spec or {}).get("fallback", [0.3, 0.3, 0.3]))
    pl.data.materials.append(m)


def look_at(cam, target):
    d = target - cam.location
    cam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()


def ground_height(sc, bx, by):
    """Highest hit of a vertical ray at Blender (bx, by) against the base geometry ("geometry" node) and the ground plane.
    Instances (trees, props) are skipped on purpose: a person stands on the ground, not on a tree."""
    best = None
    for o in bpy.data.objects:
        if o.type != "MESH" or not (o.name.startswith("geometry") or o.name.startswith("ground")):
            continue
        inv = o.matrix_world.inverted()
        origin = inv @ mathutils.Vector((bx, by, 100000.0))
        direction = (inv.to_3x3() @ mathutils.Vector((0, 0, -1))).normalized()
        ok, loc, _n, _i = o.ray_cast(origin, direction)
        if ok:
            z = (o.matrix_world @ loc).z
            best = z if best is None else max(best, z)
    return best


def make_camera(sc, spec, objs, name):
    bpy.ops.object.camera_add(location=(0, 0, 0))
    cam = bpy.context.active_object
    cam.name = "cam_" + name
    cam.data.lens = float(spec.get("lens", 35.0))
    cam.data.sensor_width = float(spec.get("sensor", 36.0))
    cam.data.clip_start = 0.1
    cam.data.clip_end = float(spec.get("clip_end", 600000.0))
    if "eye" in spec:  # standing person: height of the terrain (the model's base geometry) or the ground plane at (x, z)
        e = spec["eye"]
        x, z = e["at"]
        gy = ground_height(sc, x, -z)
        if gy is None:
            warn("camera '%s': no terrain or ground under (%g, %g); standing at height 0" % (name, x, z))
            gy = 0.0
        cam.location = mathutils.Vector((x, -z, gy + float(e.get("height", 1.7))))
        yaw, pitch = math.radians(float(e.get("yaw", 0.0))), math.radians(float(e.get("pitch", 0.0)))
        fwd = to_b((math.cos(yaw) * math.cos(pitch), math.sin(pitch), math.sin(yaw) * math.cos(pitch)))
        look_at(cam, cam.location + fwd)
    elif "location" in spec:
        cam.location = to_b(spec["location"])
        look_at(cam, to_b(spec.get("look_at", [0, 0, 0])))
    else:  # frame everything from a three-quarter view
        lo, hi = world_bounds(objs)
        c, r = (lo + hi) / 2, (hi - lo).length / 2
        fov = 2 * math.atan(cam.data.sensor_width / (2 * cam.data.lens))
        dist = r / math.sin(fov / 2) * 1.05
        cam.location = c + mathutils.Vector((0.62, -0.78, 0.55)).normalized() * dist
        look_at(cam, c)
    if spec.get("dof"):
        d = spec["dof"]
        cam.data.dof.use_dof = True
        cam.data.dof.aperture_fstop = float(d.get("fstop", 4.0))
        cam.data.dof.focus_distance = float(d.get("focus", 10.0))
    return cam


def setup_glare(sc):
    """`render.glare` (0 = off): a soft glow around very bright areas (sky between branches, sun on water), as a lens gives.
    Done in the compositor on the linear image, before the view transform."""
    amount = float(SCENE.get("render", {}).get("glare", 0.0))
    if amount <= 0.0:
        return
    try:
        sc.use_nodes = True
        t = sc.node_tree
        for n in list(t.nodes):
            t.nodes.remove(n)
        rl = t.nodes.new("CompositorNodeRLayers")
        g = t.nodes.new("CompositorNodeGlare")
        comp = t.nodes.new("CompositorNodeComposite")
        g.glare_type = "BLOOM" if "BLOOM" in [e.identifier for e in g.bl_rna.properties["glare_type"].enum_items] else "FOG_GLOW"
        for key, val in (("Threshold", float(SCENE["render"].get("glare_threshold", 2.0))), ("Strength", amount), ("Size", float(SCENE["render"].get("glare_size", 0.6)))):
            if key in g.inputs:
                g.inputs[key].default_value = val
        t.links.new(rl.outputs["Image"], g.inputs["Image"])
        t.links.new(g.outputs["Image"], comp.inputs["Image"])
        REPORT["glare"] = g.glare_type
    except Exception as e:  # the compositor's interface changes between Blender versions; a missing glow must not lose the frame
        warn("glare not applied: %s" % e)
        sc.use_nodes = False


def bake_layered(objs, mats):
    """Bake every layered material (terrain blends) into an albedo and a tangent-space normal texture through the model's UVs,
    so an engine that cannot run our layered shader still gets the real look. Writes JPEGs and fills REPORT['baked']."""
    sc = bpy.context.scene
    res = int(args.get("bake_res", "4096"))
    out_dir = args["bake_dir"]
    os.makedirs(out_dir, exist_ok=True)
    sc.cycles.samples = 4
    sc.cycles.use_denoising = False
    sc.render.bake.use_pass_direct = False
    sc.render.bake.use_pass_indirect = False
    sc.render.bake.use_pass_color = True
    sc.render.bake.margin = 8
    REPORT["baked"] = {}
    for name in sorted(mats.layered):
        mat = next(m for (n, _vc), m in mats.cache.items() if n == name)
        users = [o for o in objs if any(s.material == mat for s in o.material_slots)]
        if not users:
            warn("layered material '%s' is not on any mesh; nothing to bake" % name)
            continue
        o = users[0]
        if len(o.data.uv_layers) == 0:
            warn("layered material '%s' is on a mesh without UVs; cannot bake" % name)
            continue
        paths = {}
        for kind, btype in (("albedo", "DIFFUSE"), ("normal", "NORMAL")):
            img = bpy.data.images.new("bake_%s_%s" % (name, kind), res, res, alpha=False)
            if kind == "normal":
                img.colorspace_settings.name = "Non-Color"
            nodes = mat.node_tree.nodes
            tex = nodes.new("ShaderNodeTexImage")
            tex.image = img
            for n in nodes:
                n.select = False
            tex.select = True
            nodes.active = tex
            # Blender bakes every material slot of the object into that material's ACTIVE image node. The other slots (other
            # terrain levels, water, buildings) must not receive this bake in one of their real textures, and a slot with no image
            # node at all stops the bake with an error: each gets a throwaway target for the duration.
            scratch = bpy.data.images.new("bake_scratch", 8, 8, alpha=False)
            parked = []
            for slot in o.material_slots:
                other = slot.material
                if other is None or other == mat or not other.use_nodes:
                    continue
                on = other.node_tree.nodes
                dummy = on.new("ShaderNodeTexImage")
                dummy.image = scratch
                parked.append((on, dummy, on.active))
                for n in on:
                    n.select = False
                dummy.select = True
                on.active = dummy
            bpy.ops.object.select_all(action="DESELECT")
            o.select_set(True)
            bpy.context.view_layer.objects.active = o
            t1 = time.time()
            if kind == "albedo":
                bpy.ops.object.bake(type=btype, pass_filter={"COLOR"}, margin=8)
            else:
                bpy.ops.object.bake(type=btype, normal_space="TANGENT", margin=8)
            # JPEG: four 4096 x 4096 PNG pairs made the engine file hundreds of megabytes larger, which is what a validator and an
            # engine importer then have to hold in memory
            path = os.path.join(out_dir, "%s_%s.jpg" % (name, kind))
            img.filepath_raw = path
            img.file_format = "JPEG"
            img.save(quality=92 if kind == "albedo" else 96)
            nodes.remove(tex)
            for on, dummy, was in parked:
                on.remove(dummy)
                try:
                    on.active = was
                except ReferenceError:
                    pass
            bpy.data.images.remove(scratch)
            paths[kind] = path
            paths[kind + "_seconds"] = round(time.time() - t1, 2)
        paths["resolution"] = res
        REPORT["baked"][name] = paths


def load_model():
    """The model's meshes, from the import cache when there is one. Importing a large world takes minutes (tens of thousands of
    objects, millions of triangles); the caller names a .blend after the model's content hash (`blend_cache=`), the first run
    saves the imported scene there, and later runs with the same model open it in seconds. Materials, sky, haze and cameras are
    never cached: they are rebuilt from the scene file on every run."""
    cache = args.get("blend_cache", "")
    if cache and os.path.exists(cache):
        t = time.time()
        bpy.ops.wm.open_mainfile(filepath=cache)
        objs = [o for o in bpy.data.objects if o.type == "MESH"]
        REPORT.update({"objects": len(objs), "unique_meshes": len({o.data.name for o in objs}), "lod_nodes_dropped": 0, "import": "cache", "import_seconds": round(time.time() - t, 2)})
        return objs
    t = time.time()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    objs = import_model(args["model"])
    REPORT["import"] = "gltf"
    if cache and objs:
        try:
            os.makedirs(os.path.dirname(os.path.abspath(cache)), exist_ok=True)
            bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(cache), compress=False, check_existing=False)
        except Exception as e:  # a full disk must not lose the frame
            warn("could not save the import cache: %s" % e)
    REPORT["import_seconds"] = round(time.time() - t, 2)
    return objs


def main():
    t0 = time.time()
    objs = load_model()
    sc = bpy.context.scene
    setup_render(sc)
    if not objs:
        raise SystemExit("the model has no meshes")
    mats = bind_materials(objs)
    if args.get("bake") == "1":
        bake_layered(objs, mats)
        REPORT["total_seconds"] = round(time.time() - t0, 2)
        json.dump(REPORT, open(args["report"], "w", encoding="utf-8"), indent=1)
        print("BAKE_DONE", json.dumps(REPORT.get("baked", {})))
        return
    smooth(objs, float(SCENE.get("render", {}).get("smooth_angle", 35.0)))
    add_ground(sc, objs)
    setup_world(sc)
    shots = SCENE.get("cameras") or [dict(SCENE.get("camera", {}), name="main")]
    out = os.path.abspath(args["out"])  # Blender resolves relative render paths against something else, so never pass one
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    REPORT["frames"] = []
    only = [n for n in args.get("only", "").split(",") if n]  # only=hero,eye renders those cameras (look development)
    if only:
        shots = [s for s in shots if s.get("name") in only] or shots
    cams = [(spec.get("name", "shot%d" % i), make_camera(sc, spec, objs, spec.get("name", "shot%d" % i))) for i, spec in enumerate(shots)]
    add_fog(objs, cams)  # after the cameras: the fog boxes must not be seen by the eye-height ground lookup
    setup_glare(sc)
    for name, cam in cams:
        sc.camera = cam
        path = out if len(cams) == 1 and not only else re.sub(r"(\.png)?$", "_%s.png" % name, out, count=1)
        sc.render.filepath = path
        t1 = time.time()
        bpy.ops.render.render(write_still=True)
        REPORT["frames"].append({"name": name, "file": path, "seconds": round(time.time() - t1, 2)})
    REPORT["total_seconds"] = round(time.time() - t0, 2)
    if args.get("report"):
        json.dump(REPORT, open(args["report"], "w", encoding="utf-8"), indent=1)
    print("RENDER_DONE", json.dumps({k: REPORT[k] for k in ("objects", "unique_meshes", "triangles", "material_coverage", "unresolved", "total_seconds")}))


main()
