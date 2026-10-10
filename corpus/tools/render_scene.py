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


def add_fog(objs):
    """Haze whose density falls off exponentially with height (`fog_height` is the height over which it drops to 1/e), inside
    a big box with a volume-only material. A WORLD volume is infinite, so every shadow ray to the sun or sky is absorbed and the
    frame goes black; a box keeps the sky clear. The box is several scale heights tall, so its top is never seen as an edge."""
    L = SCENE.get("lighting", {})
    density = float(L.get("fog_density", 0.0))
    if density <= 0.0:
        return
    lo, _ = world_bounds(objs)
    scale_h, extent = max(1.0, float(L.get("fog_height", 300.0))), float(L.get("fog_extent", 6000.0))
    box_h = max(8.0 * scale_h, 800.0)
    base = lo.z - 5.0
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, base + box_h / 2))
    c = bpy.context.active_object
    c.name = "fog_layer"
    c.scale = (extent, extent, box_h)
    m = bpy.data.materials.new("fog")
    m.use_nodes = True
    t = m.node_tree
    for n in list(t.nodes):
        if n.type == "BSDF_PRINCIPLED":
            t.nodes.remove(n)
    # density = fog_density * exp(-(z - base) / scale_height)
    pos = t.nodes.new("ShaderNodeNewGeometry")
    sep = t.nodes.new("ShaderNodeSeparateXYZ")
    t.links.new(pos.outputs["Position"], sep.inputs["Vector"])
    rel = t.nodes.new("ShaderNodeMath"); rel.operation = "SUBTRACT"; rel.inputs[1].default_value = base
    t.links.new(sep.outputs["Z"], rel.inputs[0])
    neg = t.nodes.new("ShaderNodeMath"); neg.operation = "MULTIPLY"; neg.inputs[1].default_value = -1.0 / scale_h
    t.links.new(rel.outputs["Value"], neg.inputs[0])
    ex = t.nodes.new("ShaderNodeMath"); ex.operation = "EXPONENT"
    t.links.new(neg.outputs["Value"], ex.inputs[0])
    dens = t.nodes.new("ShaderNodeMath"); dens.operation = "MULTIPLY"; dens.inputs[1].default_value = density
    t.links.new(ex.outputs["Value"], dens.inputs[0])
    vs = t.nodes.new("ShaderNodeVolumeScatter")
    t.links.new(dens.outputs["Value"], vs.inputs["Density"])
    vs.inputs["Anisotropy"].default_value = float(L.get("fog_anisotropy", 0.3))
    col = L.get("fog_color", [0.8, 0.85, 0.95])
    vs.inputs["Color"].default_value = (col[0], col[1], col[2], 1.0)
    t.links.new(vs.outputs["Volume"], t.nodes["Material Output"].inputs["Volume"])
    c.data.materials.append(m)
    c.visible_shadow = False


# ---------------------------------------------------------------- materials

def tex_path(pid, kind):
    return os.path.join(CORPUS, "polyhaven", "materials", pid, "%s_%s_1k.jpg" % (pid, kind))


def flat_material(name, color, roughness=0.8, metallic=0.0, use_vertex_color=False):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    if use_vertex_color:
        vc = m.node_tree.nodes.new("ShaderNodeVertexColor")
        m.node_tree.links.new(vc.outputs["Color"], b.inputs["Base Color"])
    else:
        b.inputs["Base Color"].default_value = (color[0], color[1], color[2], 1.0)
    b.inputs["Roughness"].default_value = roughness
    b.inputs["Metallic"].default_value = metallic
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
    if var > 0:
        hsv = nt.nodes.new("ShaderNodeHueSaturation")
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


def layered_material(name, role_specs):
    """One material that mixes several library materials by per-vertex weights (`_WEIGHTS_0`, `_WEIGHTS_1`, written by
    blend_materials). Colour, roughness, metal and height are blended with the weights; one Principled BSDF shades the result."""
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
    acc = {"color": None, "rough": None, "metal": None, "height": None}

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

    bump_spec = {"bump": 0.0, "bump_distance": 0.02}
    for i, spec in enumerate(role_specs):
        if textured(spec):
            color, rough, metal, height = pbr_layer(nt, spec, coordinate_source(nt, spec))
            bump_spec["bump"] = max(bump_spec["bump"], float(spec.get("bump", 0.5)))
            rough = first_channel(nt, rough)
            metal = first_channel(nt, metal)
            height = first_channel(nt, height)
        else:
            if spec.get("polyhaven"):
                warn("layer files for %s are not on disk; flat colour used" % spec["polyhaven"])
            c = spec.get("fallback", spec.get("color", [0.5, 0.5, 0.5]))
            rgb = nt.nodes.new("ShaderNodeRGB")
            rgb.outputs[0].default_value = (c[0], c[1], c[2], 1.0)
            color, rough, metal, height = rgb.outputs[0], None, None, None
        if rough is None:
            rn = nt.nodes.new("ShaderNodeValue")
            rn.outputs[0].default_value = float(spec.get("roughness", 0.85))
            rough = rn.outputs[0]
        accumulate("color", color, wsock[i], True)
        accumulate("rough", rough, wsock[i], False)
        accumulate("metal", metal, wsock[i], False)
        accumulate("height", height, wsock[i], False)
    finish_bsdf(nt, bsdf, acc["color"], acc["rough"], acc["metal"], acc["height"], bump_spec)
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
                m = layered_material(name, role_specs)
                self.layered.add(name)
                self.stats.setdefault(name, {"resolved": all(material_spec(r) is not None for r in spec["layers"]), "textured": all(textured(r) for r in role_specs), "layered": spec["layers"], "triangles": 0})
            elif "polyhaven" in spec:
                m, textured_ok = pbr_material(name, spec)
                self.stats.setdefault(name, {"resolved": True, "textured": textured_ok, "triangles": 0})
            else:
                c = spec.get("color", spec.get("fallback", self.default_color))
                m = flat_material(name, c, spec.get("roughness", 0.8), spec.get("metallic", 0.0))
                self.stats.setdefault(name, {"resolved": True, "textured": False, "triangles": 0})
        self.cache[key] = m
        return m


def has_image_texture(m):
    return bool(m.use_nodes and m.node_tree and any(n.type == "TEX_IMAGE" for n in m.node_tree.nodes))


def base_name(n):
    return re.sub(r"\.\d{3}$", "", n)


def bind_materials(objs):
    mats = Materials()
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
                mats.stats.setdefault(name, {"resolved": True, "textured": True, "authored": True, "triangles": 0})
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


def bake_layered(objs, mats):
    """Bake every layered material (terrain blends) into an albedo and a tangent-space normal texture through the model's UVs,
    so an engine that cannot run our layered shader still gets the real look. Writes PNGs and fills REPORT['baked']."""
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
            bpy.ops.object.select_all(action="DESELECT")
            o.select_set(True)
            bpy.context.view_layer.objects.active = o
            t1 = time.time()
            if kind == "albedo":
                bpy.ops.object.bake(type=btype, pass_filter={"COLOR"}, margin=8)
            else:
                bpy.ops.object.bake(type=btype, normal_space="TANGENT", margin=8)
            path = os.path.join(out_dir, "%s_%s.png" % (name, kind))
            img.filepath_raw = path
            img.file_format = "PNG"
            img.save()
            nodes.remove(tex)
            paths[kind] = path
            paths[kind + "_seconds"] = round(time.time() - t1, 2)
        paths["resolution"] = res
        REPORT["baked"][name] = paths


def main():
    t0 = time.time()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    setup_render(sc)
    objs = import_model(args["model"])
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
    cams = [(spec.get("name", "shot%d" % i), make_camera(sc, spec, objs, spec.get("name", "shot%d" % i))) for i, spec in enumerate(shots)]
    add_fog(objs)  # after the cameras: the fog box must not be seen by the eye-height ground lookup
    for name, cam in cams:
        sc.camera = cam
        path = out if len(cams) == 1 else re.sub(r"(\.png)?$", "_%s.png" % name, out, count=1)
        sc.render.filepath = path
        t1 = time.time()
        bpy.ops.render.render(write_still=True)
        REPORT["frames"].append({"name": name, "file": path, "seconds": round(time.time() - t1, 2)})
    REPORT["total_seconds"] = round(time.time() - t0, 2)
    if args.get("report"):
        json.dump(REPORT, open(args["report"], "w", encoding="utf-8"), indent=1)
    print("RENDER_DONE", json.dumps({k: REPORT[k] for k in ("objects", "unique_meshes", "triangles", "material_coverage", "unresolved", "total_seconds")}))


main()
