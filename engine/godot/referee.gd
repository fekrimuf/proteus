extends SceneTree
# Engine referee. Usage (see `pt referee`):
#   godot --headless --path engine/godot --script referee.gd -- <model.glb> [nav=1]            import + counts + navmesh coverage
#   godot --path engine/godot --script referee.gd -- <model.glb> fps=1 [eye=x,y,z yaw=deg]     windowed: draw calls and frame rate
#   godot --path engine/godot --script referee.gd -- <model.glb> plan=<plan.json>              windowed: every view of the plan as a picture (`pt survey`)
#   godot --path engine/godot --script referee.gd -- <model.glb> plan=<plan.json> walk=1       a window to walk the world in (`pt walk`)
# Prints one line `REFEREE_JSON {...}`; exits 0 if the engine imported the file without errors, 1 otherwise.

const TERRAIN_NODE := "geometry"  # the base mesh of a Proteus export (instances are separate nodes)

var result := {"ok": false, "errors": [], "godot": Engine.get_version_info().string}
var plan := {}  # what `pt survey` and `pt walk` ask for: the views, the sun, the flagged objects, the colour of every kind
var flag_material: ShaderMaterial = null
var flag_nodes := 0


func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	if args.is_empty():
		_finish("no model path given")
		return
	var opts := {}
	for a in args.slice(1):
		var kv: PackedStringArray = a.split("=", true, 1)
		opts[kv[0]] = kv[1] if kv.size() > 1 else "1"
	var path: String = args[0]
	result["model"] = path
	if opts.has("plan"):
		var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(String(opts["plan"])))
		if not (parsed is Dictionary):
			_finish("the plan file %s is not a JSON object" % String(opts["plan"]))
			return
		plan = parsed
	result["bytes"] = FileAccess.get_file_as_bytes(path).size() if FileAccess.file_exists(path) else 0
	var t0 := Time.get_ticks_msec()
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	var err := doc.append_from_file(path, state)
	result["import_ms"] = Time.get_ticks_msec() - t0
	if err != OK:
		_finish("GLTFDocument.append_from_file failed: %s" % error_string(err))
		return
	var root := doc.generate_scene(state)
	if root == null:
		_finish("generate_scene returned null")
		return
	result["scene_ms"] = Time.get_ticks_msec() - t0
	get_root().add_child(root)
	await process_frame  # the nodes are inside the tree from here on (global transforms are valid)
	_apply_instancing(state, root)
	_apply_lod(root)
	_apply_point_colours(root)
	_count(root)
	if opts.has("nav"):
		await _navmesh(root, opts)
	if opts.has("fps"):
		await _fps(root, opts)
	result["ready_ms"] = Time.get_ticks_msec() - t0
	if opts.has("walk"):
		await _walk(root)
	elif plan.has("views"):
		await _survey(root)
	result["ok"] = (result["errors"] as Array).is_empty()
	_finish("")


func _lod_level(n: Node) -> int:
	var s := String(n.name)
	var i := s.find("__lod")
	return int(s.substr(i + 5)) if i >= 0 else -1


# The engine bridge for instancing. A tiled export gives every ground tile, prototype and level ONE node that carries
# `EXT_mesh_gpu_instancing` (three accessors: translation, rotation, scale). Godot 4.7's importer reads the node as a single
# mesh and ignores the extension, so the instances are built here: each such node becomes a MultiMeshInstance3D, which the
# engine draws as one instanced batch. The node's name and extras (the level and its distance range) are kept.
func _float_accessor(state: GLTFState, index: int, glb: PackedByteArray) -> PackedFloat32Array:
	var acc: Dictionary = (state.json["accessors"] as Array)[index]
	var view: Dictionary = (state.json["bufferViews"] as Array)[int(acc["bufferView"])]
	var off := int(view.get("byteOffset", 0)) + int(acc.get("byteOffset", 0))
	var comps := 4 if String(acc["type"]) == "VEC4" else 3
	return glb.slice(off, off + int(acc["count"]) * comps * 4).to_float32_array()


func _apply_instancing(state: GLTFState, root: Node) -> void:
	var nodes: Array = state.json.get("nodes", [])
	var glb := state.get_glb_data()
	# glTF node -> scene node by NAME: the importer swaps its own mesh nodes for MeshInstance3D at the end, so the ids it
	# recorded per glTF node are stale; the exporter's tile names are unique
	var by_name := {}
	var walk: Array[Node] = [root]
	while not walk.is_empty():
		var w: Node = walk.pop_back()
		by_name[String(w.name)] = w
		for c in w.get_children():
			walk.append(c)
	var built := {}  # "t,r,s" accessor ids -> Array of Transform3D, shared by the levels of one tile
	var flags := _flags_by_proto()
	var made := 0
	var placed := 0
	for i in nodes.size():
		var jn: Dictionary = nodes[i]
		if not jn.has("extensions") or not (jn["extensions"] as Dictionary).has("EXT_mesh_gpu_instancing"):
			continue
		var att: Dictionary = jn["extensions"]["EXT_mesh_gpu_instancing"]["attributes"]
		var old: Node = by_name.get(String(jn.get("name", "")))
		if old == null or not (old is MeshInstance3D) or (old as MeshInstance3D).mesh == null:
			(result["errors"] as Array).append("instancing: node %d has no mesh to instance" % i)
			continue
		var key := "%d,%d,%d" % [int(att["TRANSLATION"]), int(att["ROTATION"]), int(att["SCALE"])]
		if not built.has(key):
			var t := _float_accessor(state, int(att["TRANSLATION"]), glb)
			var r := _float_accessor(state, int(att["ROTATION"]), glb)
			var s := _float_accessor(state, int(att["SCALE"]), glb)
			var xf: Array = []
			for k in t.size() / 3:
				var b := Basis(Quaternion(r[k * 4], r[k * 4 + 1], r[k * 4 + 2], r[k * 4 + 3])).scaled_local(Vector3(s[k * 3], s[k * 3 + 1], s[k * 3 + 2]))
				xf.append(Transform3D(b, Vector3(t[k * 3], t[k * 3 + 1], t[k * 3 + 2])))
			built[key] = xf
		var xforms: Array = built[key]
		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = (old as MeshInstance3D).mesh
		# the flagged objects of this prototype (float, hang, drowned, buried), by where they stand: they get a colour of their own
		var mine: Variant = null
		if jn.has("extras") and (jn["extras"] as Dictionary).has("proteus_proto"):
			mine = flags.get(int(jn["extras"]["proteus_proto"]))
		mm.use_custom_data = mine != null
		mm.instance_count = xforms.size()
		var flagged := 0
		for k in xforms.size():
			mm.set_instance_transform(k, xforms[k])
			if mine != null:
				var col: Variant = (mine as Dictionary).get(_flag_key((xforms[k] as Transform3D).origin))
				mm.set_instance_custom_data(k, col if col != null else Color(0, 0, 0, 0))
				flagged += 1 if col != null else 0
		var mmi := MultiMeshInstance3D.new()
		mmi.multimesh = mm
		if flagged > 0:
			mmi.material_overlay = _flag_material()
			flag_nodes += 1
		var nm := String(old.name)
		if old.has_meta("extras"):
			mmi.set_meta("extras", old.get_meta("extras"))
		var parent := old.get_parent()
		parent.remove_child(old)
		old.queue_free()
		mmi.name = nm
		parent.add_child(mmi)
		made += 1
		placed += xforms.size()
	result["instancing"] = {"nodes": made, "instances_with_levels": placed, "nodes_with_flagged": flag_nodes}


# The engine bridge for LODs: the exporter writes one node per level with its distance range in the node extras
# (`proteus_lod_range`, metres); here they become visibility ranges, which is how an engine switches levels by distance.
func _apply_lod(root: Node) -> void:
	var applied := 0
	var parented := 0
	var by_name := {}
	var all: Array[Node] = [root]
	while not all.is_empty():
		var w: Node = all.pop_back()
		by_name[String(w.name)] = w
		for c in w.get_children():
			all.append(c)
	var stack: Array[Node] = [root]
	while not stack.is_empty():
		var n: Node = stack.pop_back()
		for c in n.get_children():
			stack.append(c)
		# The ground receives shadows and does not cast them: every shadow cascade would draw the whole terrain again (it is
		# most of a world's triangles), for shadows of hills that a baked or far shadow gives a shipped game anyway.
		if n is MeshInstance3D and String(n.name).begins_with(TERRAIN_NODE + "__terrain"):
			(n as GeometryInstance3D).cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
			result["ground_nodes_without_shadow"] = int(result.get("ground_nodes_without_shadow", 0)) + 1
		if n is GeometryInstance3D and n.has_meta("extras"):
			var ex: Variant = n.get_meta("extras")
			if ex is Dictionary and ex.has("proteus_lod_range"):
				var r: Array = ex["proteus_lod_range"]
				(n as GeometryInstance3D).visibility_range_begin = float(r[0])
				(n as GeometryInstance3D).visibility_range_end = float(r[1])
				(n as GeometryInstance3D).visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_DISABLED
				applied += 1
				# ground cover (grass, ferns, pebbles) does not cast shadows: it is most of what a world draws and its shadows are lost in itself
				if ex.get("proteus_cover", false):
					(n as GeometryInstance3D).cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
				# a near tile is drawn exactly when its far tile (the last level, one batch for 4 x 4 near tiles) is not
				if ex.has("proteus_hlod_parent") and by_name.has(String(ex["proteus_hlod_parent"])):
					(n as GeometryInstance3D).visibility_parent = n.get_path_to(by_name[String(ex["proteus_hlod_parent"])])
					parented += 1
	result["lod_ranges_applied"] = applied
	result["hlod_parents_set"] = parented


# The engine bridge for point colours. The exporter writes the point colour `Cd` as COLOR_0: a far tree's shell takes its colour
# from it, and so does a ground whose layers were not baked (`pt survey`). Godot's importer leaves the material's switch for it
# off, so every shell and every such ground drew white. It is switched on for materials that have no colour map of their own (a
# textured material keeps its look).
func _apply_point_colours(root: Node) -> void:
	var seen := {}
	var switched := 0
	var stack: Array[Node] = [root]
	while not stack.is_empty():
		var n: Node = stack.pop_back()
		for c in n.get_children():
			stack.append(c)
		var mesh: Mesh = null
		if n is MeshInstance3D:
			mesh = (n as MeshInstance3D).mesh
		elif n is MultiMeshInstance3D and (n as MultiMeshInstance3D).multimesh != null:
			mesh = (n as MultiMeshInstance3D).multimesh.mesh
		if mesh == null or not (mesh is ArrayMesh) or seen.has(mesh.get_rid()):
			continue
		seen[mesh.get_rid()] = true
		for s in mesh.get_surface_count():
			if ((mesh as ArrayMesh).surface_get_format(s) & Mesh.ARRAY_FORMAT_COLOR) == 0:
				continue
			var m := mesh.surface_get_material(s)
			if m is BaseMaterial3D and (m as BaseMaterial3D).albedo_texture == null and not (m as BaseMaterial3D).vertex_color_use_as_albedo:
				(m as BaseMaterial3D).vertex_color_use_as_albedo = true
				switched += 1
	result["materials_with_point_colour"] = switched


func _count(root: Node) -> void:
	var nodes := 0
	var mesh_instances := 0
	var instanced_nodes := 0
	var instances := 0
	var lower_lod_nodes := 0
	var triangles := 0
	var unique_meshes := {}
	var materials := {}
	var textures := {}
	var tex_bytes := 0
	var aabb := AABB()
	var first := true
	var stack: Array[Node] = [root]
	while not stack.is_empty():
		var n: Node = stack.pop_back()
		nodes += 1
		for c in n.get_children():
			stack.append(c)
		if n is MeshInstance3D:
			var mi := n as MeshInstance3D
			if mi.mesh == null:
				continue
			mesh_instances += 1
			var tris := 0
			for s in mi.mesh.get_surface_count():
				var idx: int = (mi.mesh as ArrayMesh).surface_get_array_index_len(s) if mi.mesh is ArrayMesh else 0
				tris += idx / 3
				var m := mi.mesh.surface_get_material(s)
				if m != null:
					materials[m.get_rid()] = true
					if m is BaseMaterial3D:
						for prop in [m.albedo_texture, m.normal_texture, m.roughness_texture, m.metallic_texture, m.ao_texture, m.emission_texture]:
							if prop != null and not textures.has(prop.get_rid()):
								textures[prop.get_rid()] = true
								tex_bytes += prop.get_width() * prop.get_height() * 4 * 4 / 3
			var lod := _lod_level(n)
			if lod <= 0:
				triangles += tris  # the world as authored (level 0); lower levels are counted separately
			else:
				lower_lod_nodes += 1
			unique_meshes[mi.mesh.get_rid()] = tris
			var box := mi.global_transform * mi.get_aabb()
			aabb = box if first else aabb.merge(box)
			first = false
		elif n is MultiMeshInstance3D:
			# a tile of a tiled export: one node draws every instance of one prototype in one ground tile (EXT_mesh_gpu_instancing)
			var mm := (n as MultiMeshInstance3D).multimesh
			if mm == null or mm.mesh == null:
				continue
			instanced_nodes += 1
			var mtris := 0
			for s in mm.mesh.get_surface_count():
				mtris += ((mm.mesh as ArrayMesh).surface_get_array_index_len(s) if mm.mesh is ArrayMesh else 0) / 3
				var m2 := mm.mesh.surface_get_material(s)
				if m2 != null:
					materials[m2.get_rid()] = true
					if m2 is BaseMaterial3D:
						for prop in [m2.albedo_texture, m2.normal_texture, m2.roughness_texture, m2.metallic_texture, m2.ao_texture, m2.emission_texture]:
							if prop != null and not textures.has(prop.get_rid()):
								textures[prop.get_rid()] = true
								tex_bytes += prop.get_width() * prop.get_height() * 4 * 4 / 3
			if _lod_level(n) <= 0:
				triangles += mtris * mm.instance_count
				instances += mm.instance_count
			else:
				lower_lod_nodes += 1
			unique_meshes[mm.mesh.get_rid()] = mtris
			var box2 := (n as MultiMeshInstance3D).global_transform * (n as MultiMeshInstance3D).get_aabb()
			aabb = box2 if first else aabb.merge(box2)
			first = false
	var unique_tris := 0
	for v in unique_meshes.values():
		unique_tris += v
	result["nodes"] = nodes
	result["mesh_instances"] = mesh_instances
	result["instanced_nodes"] = instanced_nodes
	result["instances_in_tiles"] = instances
	result["lower_lod_nodes"] = lower_lod_nodes
	result["unique_meshes"] = unique_meshes.size()
	result["triangles_total"] = triangles
	result["triangles_unique"] = unique_tris
	result["materials"] = materials.size()
	result["textures"] = textures.size()
	result["texture_mb_estimate"] = snappedf(tex_bytes / 1048576.0, 0.1)
	result["bounds_min"] = [aabb.position.x, aabb.position.y, aabb.position.z]
	result["bounds_size"] = [aabb.size.x, aabb.size.y, aabb.size.z]


# Navigation coverage of the terrain: baked navmesh area divided by the area of terrain triangles that are walkable by slope.
func _navmesh(root: Node, opts: Dictionary) -> void:
	var bases: Array = []
	for c in root.get_children():
		if c is MeshInstance3D and String(c.name).begins_with(TERRAIN_NODE) and (c as MeshInstance3D).mesh != null:
			bases.append(c)
	if bases.is_empty():
		(result["errors"] as Array).append("navmesh: no '%s' node (the base geometry) to bake" % TERRAIN_NODE)
		return
	var terrain: MeshInstance3D = bases[0]
	var max_slope := float(opts.get("slope", "40"))
	var nav := NavigationMesh.new()
	nav.cell_size = float(opts.get("cell", "1.0"))
	nav.cell_height = 0.5
	nav.agent_radius = float(opts.get("radius", "1.0"))
	nav.agent_height = 1.8
	nav.agent_max_climb = 0.6
	nav.agent_max_slope = max_slope
	# A world with a far ring is hundreds of square kilometres; the navigation mesh is baked and measured over the PLAYABLE window only
	# (`nav_box=x0,z0,x1,z1` in metres), with a margin so the bake does not end in a wall at the edge of the measured ground.
	var box := Rect2()
	var has_box := false
	if opts.has("nav_box"):
		var bp := String(opts["nav_box"]).split(",")
		if bp.size() == 4:
			box = Rect2(float(bp[0]), float(bp[1]), float(bp[2]) - float(bp[0]), float(bp[3]) - float(bp[1]))
			has_box = true
	# The base mesh has one surface per material: the ground people walk on, but also water, a merged distant forest, buildings.
	# `nav_mats=a,b` names the ground's materials; without it every surface whose material starts with "terrain" or "ground"
	# counts, and a mesh with a single surface is taken whole.
	var want: PackedStringArray = String(opts.get("nav_mats", "")).split(",", false)
	var tv := PackedVector3Array()
	var ti := PackedInt32Array()
	var used: Array = []
	# (a tiled export writes the base mesh as one node per material, `geometry__<material>`; an older file has one node with a
	# surface per material: both are read here)
	var n_surf := 0
	for b in bases:
		n_surf += (b as MeshInstance3D).mesh.get_surface_count()
	for b in bases:
		var bm: Mesh = (b as MeshInstance3D).mesh
		for s in bm.get_surface_count():
			var mat := bm.surface_get_material(s)
			var nm := String(mat.resource_name) if mat != null else ""
			var take := n_surf == 1
			if not want.is_empty():
				take = take or want.has(nm)
			else:
				take = take or nm.begins_with("terrain") or nm.begins_with("ground")
			if not take:
				continue
			var arrays := bm.surface_get_arrays(s)
			var base := tv.size()
			tv.append_array(arrays[Mesh.ARRAY_VERTEX])
			var idx: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
			if base == 0:
				ti = idx
			else:
				var start := ti.size()
				ti.append_array(idx)
				for k in range(start, ti.size()):
					ti[k] += base
			used.append(nm)
	if ti.is_empty():
		(result["errors"] as Array).append("navmesh: no ground surface on '%s' (materials wanted: %s)" % [TERRAIN_NODE, ",".join(want)])
		return
	# Recast indexes a baked mesh with 16 bits, so one bake holds about 65 000 vertices: a square kilometre and a half of real
	# terrain overflows it and comes back EMPTY (0 polygons, no error). The window is therefore baked in tiles of at most
	# `nav_tile` metres (400 by default): each tile gets the ground inside it plus a margin, and a filter box so its polygons
	# stop at its own edge; the tiles' polygons are added up.
	var tile_m := float(opts.get("nav_tile", "400"))
	const MARGIN := 24.0
	var nx := 1
	var nz := 1
	if has_box:
		nx = maxi(1, int(ceil(box.size.x / tile_m)))
		nz = maxi(1, int(ceil(box.size.y / tile_m)))
	var sx := box.size.x / nx
	var sz := box.size.y / nz
	var tile_faces: Array = []  # plain Arrays: a Packed array stored in an Array is copied by a cast
	for i in nx * nz:
		tile_faces.append([])
	var outer := box.grow(MARGIN)
	for t in range(0, ti.size(), 3):
		var fa := tv[ti[t]]
		var fb := tv[ti[t + 1]]
		var fc := tv[ti[t + 2]]
		if not has_box:
			(tile_faces[0] as Array).append_array([fa, fb, fc])
			continue
		var cen := (fa + fb + fc) / 3.0
		if not outer.has_point(Vector2(cen.x, cen.z)):
			continue
		var ix0 := clampi(int(floor((cen.x - MARGIN - box.position.x) / sx)), 0, nx - 1)
		var ix1 := clampi(int(floor((cen.x + MARGIN - box.position.x) / sx)), 0, nx - 1)
		var iz0 := clampi(int(floor((cen.z - MARGIN - box.position.y) / sz)), 0, nz - 1)
		var iz1 := clampi(int(floor((cen.z + MARGIN - box.position.y) / sz)), 0, nz - 1)
		for ix in range(ix0, ix1 + 1):
			for iz in range(iz0, iz1 + 1):
				(tile_faces[ix * nz + iz] as Array).append_array([fa, fb, fc])
	var all_tris: Array = []
	var area := 0.0
	var poly_count := 0
	var bake_ms := 0
	for ix in nx:
		for iz in nz:
			var arr: Array = tile_faces[ix * nz + iz]
			if arr.is_empty():
				continue
			var src := NavigationMeshSourceGeometryData3D.new()
			src.add_faces(PackedVector3Array(arr), terrain.global_transform)
			var tile_nav := NavigationMesh.new()
			tile_nav.cell_size = nav.cell_size
			tile_nav.cell_height = nav.cell_height
			tile_nav.agent_radius = nav.agent_radius
			tile_nav.agent_height = nav.agent_height
			tile_nav.agent_max_climb = nav.agent_max_climb
			tile_nav.agent_max_slope = nav.agent_max_slope
			if has_box:
				# the box is as tall as the tile's own ground and no taller: the bake stores heights as 16-bit steps of the cell
				# height above the box's floor, so a box "from minus to plus 100 km" puts every vertex at a garbage height
				var ylo := 1.0e30
				var yhi := -1.0e30
				for v in arr:
					ylo = minf(ylo, (v as Vector3).y)
					yhi = maxf(yhi, (v as Vector3).y)
				tile_nav.filter_baking_aabb = AABB(Vector3(box.position.x + sx * ix, ylo - 5.0, box.position.y + sz * iz), Vector3(sx, yhi - ylo + 10.0, sz))
			var t0 := Time.get_ticks_msec()
			NavigationServer3D.bake_from_source_geometry_data(tile_nav, src)
			bake_ms += Time.get_ticks_msec() - t0
			var verts := tile_nav.get_vertices()
			poly_count += tile_nav.get_polygon_count()
			for p in tile_nav.get_polygon_count():
				var poly := tile_nav.get_polygon(p)
				for k in range(1, poly.size() - 1):
					var tri := [verts[poly[0]], verts[poly[k]], verts[poly[k + 1]]]
					area += absf((tri[1] - tri[0]).cross(tri[2] - tri[0]).y) * 0.5  # horizontal area
					all_tris.append(tri)
	# expected: horizontal area of terrain triangles whose slope is within the limit, and the INTENDED walkable ground
	# (slope under `intended` degrees, 25 by default): the part of the world a designer means people to walk on
	var walkable := 0.0
	var total := 0.0
	var cos_lim := cos(deg_to_rad(max_slope))
	var cos_int := cos(deg_to_rad(float(opts.get("intended", "25"))))
	var intended_tris: PackedInt32Array = PackedInt32Array()
	var intended_cum: PackedFloat32Array = PackedFloat32Array()
	var intended_area := 0.0
	for t in range(0, ti.size(), 3):
		var a := tv[ti[t]]
		var b := tv[ti[t + 1]]
		var c := tv[ti[t + 2]]
		if has_box:
			var cen2 := (a + b + c) / 3.0
			if not box.has_point(Vector2(cen2.x, cen2.z)):
				continue
		var nrm := (b - a).cross(c - a)
		var len := nrm.length()
		if len < 1e-9:
			continue
		var horiz := absf(nrm.y) * 0.5
		total += horiz
		var up := absf(nrm.y) / len
		if up >= cos_lim:
			walkable += horiz
		if up >= cos_int:
			intended_area += horiz
			intended_tris.append(t)
			intended_cum.append(intended_area)
	# Does the baked navigation mesh reach the intended ground? Sample points on it (area-weighted) and test each against the
	# baked navigation triangles in the XZ plane (grid lookup); within the agent radius plus a cell of a triangle counts as reached
	# (the baked mesh is pulled in from every edge by the agent radius, so ground right at a cliff foot is never walkable).
	var nav_tris: Array = []
	var grid := {}
	const G := 8.0
	for tri in all_tris:
		nav_tris.append(tri)
		var lo_x := int(floor(minf(tri[0].x, minf(tri[1].x, tri[2].x)) / G))
		var hi_x := int(floor(maxf(tri[0].x, maxf(tri[1].x, tri[2].x)) / G))
		var lo_z := int(floor(minf(tri[0].z, minf(tri[1].z, tri[2].z)) / G))
		var hi_z := int(floor(maxf(tri[0].z, maxf(tri[1].z, tri[2].z)) / G))
		for gx in range(lo_x, hi_x + 1):
			for gz in range(lo_z, hi_z + 1):
				var key := Vector2i(gx, gz)
				if not grid.has(key):
					grid[key] = []  # a plain Array (by reference); a Packed array would be copied by the cast below
				(grid[key] as Array).append(nav_tris.size() - 1)
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	var samples := int(opts.get("samples", "20000"))
	var tol := nav.agent_radius + nav.cell_size
	var reached := 0
	for s in samples:
		var target := rng.randf() * intended_area
		var lo := 0
		var hi := intended_cum.size() - 1
		while lo < hi:
			var mid := (lo + hi) / 2
			if intended_cum[mid] < target:
				lo = mid + 1
			else:
				hi = mid
		var t := intended_tris[lo]
		var r1 := rng.randf()
		var r2 := rng.randf()
		if r1 + r2 > 1.0:
			r1 = 1.0 - r1
			r2 = 1.0 - r2
		var pt := tv[ti[t]] + (tv[ti[t + 1]] - tv[ti[t]]) * r1 + (tv[ti[t + 2]] - tv[ti[t]]) * r2
		var found := false
		var cx := int(floor(pt.x / G))
		var cz := int(floor(pt.z / G))
		for dx in range(-1, 2):
			for dz in range(-1, 2):
				var cell: Variant = grid.get(Vector2i(cx + dx, cz + dz))
				if cell == null:
					continue
				for ni in (cell as Array):
					var tr: Array = nav_tris[ni]
					if absf(float(tr[0].y) - pt.y) > 3.0 and absf(float(tr[1].y) - pt.y) > 3.0 and absf(float(tr[2].y) - pt.y) > 3.0:
						continue
					if _dist_xz_to_tri(pt, tr[0], tr[1], tr[2]) <= tol:
						found = true
						break
				if found:
					break
			if found:
				break
		if found:
			reached += 1
	result["navmesh"] = {
		"polygons": poly_count, "tiles": nx * nz, "area_m2": snappedf(area, 1.0), "walkable_terrain_m2": snappedf(walkable, 1.0),
		"terrain_m2": snappedf(total, 1.0), "coverage": snappedf(area / maxf(walkable, 1.0), 0.001), "bake_ms": bake_ms,
		"max_slope_deg": max_slope, "agent_radius": nav.agent_radius,
		"intended_slope_deg": float(opts.get("intended", "25")), "intended_m2": snappedf(intended_area, 1.0),
		"coverage_intended": snappedf(float(reached) / maxf(samples, 1), 0.001), "samples": samples, "ground_materials": used,
	}


# Horizontal (XZ) distance from a point to a triangle; 0 when the point is inside it.
func _dist_xz_to_tri(p: Vector3, a: Vector3, b: Vector3, c: Vector3) -> float:
	var pp := Vector2(p.x, p.z)
	var va := Vector2(a.x, a.z)
	var vb := Vector2(b.x, b.z)
	var vc := Vector2(c.x, c.z)
	var d1 := (vb - va).cross(pp - va)
	var d2 := (vc - vb).cross(pp - vb)
	var d3 := (va - vc).cross(pp - vc)
	if (d1 >= 0 and d2 >= 0 and d3 >= 0) or (d1 <= 0 and d2 <= 0 and d3 <= 0):
		return 0.0
	var best := 1e9
	for e in [[va, vb], [vb, vc], [vc, va]]:
		var s: Vector2 = e[1] - e[0]
		var t := clampf((pp - e[0]).dot(s) / maxf(s.length_squared(), 1e-9), 0.0, 1.0)
		best = minf(best, (pp - (e[0] + s * t)).length())
	return best


# Height of the terrain (the "geometry" node) under (x, z), by ray against its triangles; -1e9 if there is none.
func _ground_y(root: Node, x: float, z: float) -> float:
	# the highest hit over every base node (the terrain levels, the water, a bridge): what the render's camera stands on too
	var best := -1e9
	for c in root.get_children():
		if c is MeshInstance3D and String(c.name).begins_with(TERRAIN_NODE) and c.mesh != null:
			var mi := c as MeshInstance3D
			var box := mi.get_aabb()
			if x < box.position.x or x > box.end.x or z < box.position.z or z > box.end.z:
				continue
			var tm: TriangleMesh = mi.mesh.generate_triangle_mesh()
			var hit := tm.intersect_ray(Vector3(x, 100000.0, z), Vector3(0, -1, 0))
			if not hit.is_empty():
				best = maxf(best, float(hit["position"].y))
	return best


# Windowed: a camera, a sun and a sky, a few seconds of frames; reports frame rate and draw calls and can save the picture.
# Options: eye_at=x,z height=1.7 yaw=deg pitch=deg (the camera stands on the terrain), or eye=x,y,z; size=1280x720; shot=path.png
func _fps(root: Node, opts: Dictionary) -> void:
	var size := Vector2i(1280, 720)
	if opts.has("size"):
		var sz: PackedStringArray = String(opts["size"]).split("x")
		size = Vector2i(int(sz[0]), int(sz[1]))
	get_root().size = size
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)  # measure what the machine can do, not the monitor's refresh rate
	var eye := Vector3(-330, 40, 30)
	if opts.has("eye_at"):
		var p: PackedStringArray = String(opts["eye_at"]).split(",")
		var gy := _ground_y(root, float(p[0]), float(p[1]))
		eye = Vector3(float(p[0]), (gy if gy > -1e8 else 0.0) + float(opts.get("height", "1.7")), float(p[1]))
	elif opts.has("eye"):
		var p2: PackedStringArray = String(opts["eye"]).split(",")
		eye = Vector3(float(p2[0]), float(p2[1]), float(p2[2]))
	# toward the sun at azimuth 150, elevation 27 (the alpine style)
	var cam := _stage(root, 150.0, 27.0, float(opts.get("shadow_m", "150")))
	_aim(cam, eye, float(opts.get("yaw", "0")), float(opts.get("pitch", "0")), float(opts.get("lens", "28")), size)
	var frames := int(opts.get("frames", "90"))
	for i in 20:  # warm-up
		await process_frame
	var t0 := Time.get_ticks_usec()
	for i in frames:
		await process_frame
	var dt := (Time.get_ticks_usec() - t0) / 1.0e6
	var rs := RenderingServer
	result["fps"] = {
		"frames": frames, "seconds": snappedf(dt, 0.01), "fps": snappedf(frames / maxf(dt, 1e-6), 0.1),
		"draw_calls": rs.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME),
		"primitives": rs.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME),
		"objects": rs.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_OBJECTS_IN_FRAME),
		"video_mem_mb": snappedf(rs.get_rendering_info(RenderingServer.RENDERING_INFO_VIDEO_MEM_USED) / 1048576.0, 0.1),
		"adapter": RenderingServer.get_video_adapter_name(), "camera": [eye.x, eye.y, eye.z], "size": [size.x, size.y],
	}
	if opts.has("shot"):
		await RenderingServer.frame_post_draw
		var img := get_root().get_texture().get_image()
		result["shot"] = String(opts["shot"]) if img.save_png(String(opts["shot"])) == OK else "save failed"


# A camera, the sun and a sky. The sun stands at `azimuth` degrees (0 looks along +X and grows toward +Z, like a camera's yaw)
# and `elevation` degrees above the horizon. `ambient` lifts the light in the shade: a survey looks for flaws there too.
func _stage(root: Node, azimuth: float, elevation: float, shadow_m: float, ambient: float = 1.0) -> Camera3D:
	var cam := Camera3D.new()
	cam.far = 6000.0
	root.add_child(cam)
	cam.current = true
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-elevation, 90.0 - azimuth, 0)
	sun.shadow_enabled = true
	# what a game ships with in a forest: shadows to 150 m in two cascades (every cascade draws the casters again)
	sun.directional_shadow_mode = DirectionalLight3D.SHADOW_PARALLEL_2_SPLITS
	sun.directional_shadow_max_distance = shadow_m
	root.add_child(sun)
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = Sky.new()
	env.sky.sky_material = ProceduralSkyMaterial.new()
	env.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	env.ambient_light_energy = ambient
	var we := WorldEnvironment.new()
	we.environment = env
	root.add_child(we)
	return cam


# Stand the camera at `eye`, looking along `yaw` and `pitch` (degrees) through a lens of `lens` mm on a 36 mm sensor, like the
# Blender render and `pt pick`.
func _aim(cam: Camera3D, eye: Vector3, yaw: float, pitch: float, lens: float, size: Vector2i) -> void:
	cam.fov = rad_to_deg(2.0 * atan((18.0 / lens) * (float(size.y) / float(size.x))))  # Godot's fov is vertical
	cam.global_position = eye
	# Godot's camera looks down -Z; our yaw 0 looks along +X and grows toward +Z
	cam.rotation_degrees = Vector3(pitch, -90.0 - yaw, 0)


func _plan_size() -> Vector2i:
	var s: Array = plan.get("size", [1280, 720])
	return Vector2i(int(s[0]), int(s[1]))


# Where an object stands, to the centimetre: the key the flagged objects of the plan are found by.
func _flag_key(p: Vector3) -> Vector3i:
	return Vector3i(roundi(p.x * 100.0), roundi(p.y * 100.0), roundi(p.z * 100.0))


# The plan's flagged objects, `[prototype, x, y, z, r, g, b]` each, as prototype -> {place -> colour}.
func _flags_by_proto() -> Dictionary:
	var out := {}
	for f in plan.get("flags", []):
		var a: Array = f
		var p := int(a[0])
		if not out.has(p):
			out[p] = {}
		(out[p] as Dictionary)[_flag_key(Vector3(a[1], a[2], a[3]))] = Color(a[4], a[5], a[6], 1.0)
	return out


# Drawn over a flagged object: its colour, flat and unlit, so it shows from every side and in shadow. `on` switches it.
func _flag_material() -> ShaderMaterial:
	if flag_material == null:
		var sh := Shader.new()
		sh.code = "shader_type spatial;\nrender_mode unshaded, cull_disabled;\nuniform float on = 0.0;\nvarying vec4 flag;\nvoid vertex() {\n\tflag = INSTANCE_CUSTOM;\n}\nvoid fragment() {\n\tALBEDO = flag.rgb;\n\tALPHA = flag.a * on;\n}\n"
		flag_material = ShaderMaterial.new()
		flag_material.shader = sh
	return flag_material


func _show_flags(on: bool) -> void:
	if flag_material != null:
		flag_material.set_shader_parameter("on", 1.0 if on else 0.0)


const KIND_SHADER := "shader_type spatial;\nrender_mode cull_disabled;\nuniform vec3 kind : source_color;\nuniform sampler2D cut : source_color;\nuniform bool has_cut = false;\nvoid fragment() {\n\tif (has_cut && texture(cut, UV).a < 0.5) {\n\t\tdiscard;\n\t}\n\tALBEDO = kind;\n\tROUGHNESS = 1.0;\n}\n"


# Paint every kind of object in one colour of its own, lit, so shapes still read: instanced prototypes by their number (the
# plan's `kinds`), base surfaces by their material's name (`base_kinds`). A leaf card keeps its cut-out. Returns what was
# swapped, for `_restore_kinds`.
func _paint_kinds(root: Node) -> Array:
	var shader := Shader.new()
	shader.code = KIND_SHADER
	var kinds: Dictionary = plan.get("kinds", {})
	var base_kinds: Dictionary = plan.get("base_kinds", {})
	var swapped: Array = []
	var seen := {}
	var stack: Array[Node] = [root]
	while not stack.is_empty():
		var n: Node = stack.pop_back()
		for c in n.get_children():
			stack.append(c)
		var mesh: Mesh = null
		var colour: Variant = null
		if n is MultiMeshInstance3D and (n as MultiMeshInstance3D).multimesh != null:
			mesh = (n as MultiMeshInstance3D).multimesh.mesh
			var ex: Variant = n.get_meta("extras") if n.has_meta("extras") else null
			if ex is Dictionary and ex.has("proteus_proto"):
				colour = kinds.get(str(int(ex["proteus_proto"])))
		elif n is MeshInstance3D:
			mesh = (n as MeshInstance3D).mesh
		if mesh == null or seen.has(mesh.get_rid()):
			continue
		seen[mesh.get_rid()] = true
		for s in mesh.get_surface_count():
			var orig := mesh.surface_get_material(s)
			var c2: Variant = colour
			if c2 == null and orig != null:
				c2 = base_kinds.get(String(orig.resource_name))
			if c2 == null:
				continue
			var a: Array = c2
			var m := ShaderMaterial.new()
			m.shader = shader
			m.set_shader_parameter("kind", Color(a[0], a[1], a[2]))
			if orig is BaseMaterial3D and (orig as BaseMaterial3D).transparency != BaseMaterial3D.TRANSPARENCY_DISABLED and (orig as BaseMaterial3D).albedo_texture != null:
				m.set_shader_parameter("cut", (orig as BaseMaterial3D).albedo_texture)
				m.set_shader_parameter("has_cut", true)
			swapped.append([mesh, s, orig])
			mesh.surface_set_material(s, m)
	return swapped


func _restore_kinds(swapped: Array) -> void:
	for e in swapped:
		(e[0] as Mesh).surface_set_material(int(e[1]), e[2])


# One picture to disk. The engine's PNG writer takes a fifth of a second a picture, most of a survey's time, so with the plan's
# `raw` the plain bytes go out (`<file>.rgb`, three bytes a pixel) and `pt` writes the PNG, on every core.
func _save(img: Image, file: String) -> bool:
	if not bool(plan.get("raw", false)):
		return img.save_png(file) == OK
	img.convert(Image.FORMAT_RGB8)
	var f := FileAccess.open(file + ".rgb", FileAccess.WRITE)
	if f == null:
		return false
	f.store_buffer(img.get_data())
	f.close()
	return true


# `pt survey`: every view of the plan as a picture, in one launch. A view is {"name", "eye": [x, y, z], "yaw", "pitch", "lens",
# "file", "pass"}. Passes: "lit" (the world as the engine draws it), "flags" (the same, every flagged object in its colour) and
# "kinds" (every kind of object in one colour of its own).
func _survey(root: Node) -> void:
	var size := _plan_size()
	get_root().size = size
	DisplayServer.window_set_title("Proteus survey")
	DisplayServer.window_set_flag(DisplayServer.WINDOW_FLAG_NO_FOCUS, true)  # the owner is working: do not take the keyboard
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	var sun: Dictionary = plan.get("sun", {})
	var cam := _stage(root, float(sun.get("azimuth", 150.0)), float(sun.get("elevation", 27.0)), float(plan.get("shadow_m", 150.0)), float(plan.get("ambient", 1.6)))
	var views: Array = plan.get("views", [])
	var settle := int(plan.get("settle_frames", 2))
	var done := 0
	var failed: Array = []
	var t0 := Time.get_ticks_msec()
	var first := true
	for pass_name in ["lit", "flags", "kinds"]:
		var mine: Array = views.filter(func(v): return String(v.get("pass", "lit")) == pass_name)
		if mine.is_empty():
			continue
		_show_flags(pass_name == "flags")
		var swapped: Array = _paint_kinds(root) if pass_name == "kinds" else []
		for v in mine:
			var e: Array = v["eye"]
			_aim(cam, Vector3(e[0], e[1], e[2]), float(v.get("yaw", 0.0)), float(v.get("pitch", 0.0)), float(v.get("lens", 28.0)), size)
			# the first view also waits for the shaders and textures the engine prepares on first use
			for i in (20 if first else settle):
				await process_frame
			first = false
			await RenderingServer.frame_post_draw
			var img := get_root().get_texture().get_image()
			if img != null and _save(img, String(v["file"])):
				done += 1
				size = Vector2i(img.get_width(), img.get_height())
			else:
				failed.append(String(v.get("name", "?")))
		_restore_kinds(swapped)
	_show_flags(false)
	var ms := Time.get_ticks_msec() - t0
	result["survey"] = {
		"views": done, "failed": failed, "ms": ms, "ms_per_view": snappedf(float(ms) / maxf(done, 1), 0.1), "size": [size.x, size.y],
		"video_mem_mb": snappedf(RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_VIDEO_MEM_USED) / 1048576.0, 0.1),
		"adapter": RenderingServer.get_video_adapter_name(),
	}
	if not failed.is_empty():
		(result["errors"] as Array).append("survey: %d view(s) were not written" % failed.size())


# `pt walk`: a window to move through the world in and point at what looks wrong. Everything the owner notes goes to the plan's
# `notes` file, one JSON line each; `pt walk` names the objects afterwards. The plan's `ready` file tells `pt` the world is up.
func _walk(root: Node) -> void:
	var size := _plan_size()
	get_root().size = size
	auto_accept_quit = false  # closing the window ends the walk here, so the result line is still printed
	DisplayServer.window_set_title("Proteus walk-through   (H: help, Esc: close)")
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_ENABLED)  # the monitor's rate is plenty: leave the rest of the card alone
	Engine.max_fps = 60
	var sun: Dictionary = plan.get("sun", {})
	var cam := _stage(root, float(sun.get("azimuth", 150.0)), float(sun.get("elevation", 27.0)), float(plan.get("shadow_m", 150.0)), float(plan.get("ambient", 1.6)))
	var walker: Node = (load("res://walk.gd") as GDScript).new()
	var bases: Array = []
	for c in root.get_children():
		if c is MeshInstance3D and String(c.name).begins_with(TERRAIN_NODE + "__terrain") and (c as MeshInstance3D).mesh != null:
			bases.append(c)
	walker.call("setup", cam, plan, bases, Callable(self, "_show_flags"))
	get_root().add_child(walker)
	if plan.has("ready"):
		var f := FileAccess.open(String(plan["ready"]), FileAccess.WRITE)
		if f != null:
			f.store_line("ready")
			f.close()
	var t0 := Time.get_ticks_msec()
	await walker.closed
	result["walk"] = {"notes": int(walker.get("notes")), "seconds": snappedf((Time.get_ticks_msec() - t0) / 1000.0, 0.1)}


func _finish(error: String) -> void:
	if error != "":
		(result["errors"] as Array).append(error)
	print("REFEREE_JSON ", JSON.stringify(result))
	quit(0 if (result["errors"] as Array).is_empty() else 1)
