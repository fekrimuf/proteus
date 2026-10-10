extends SceneTree
# Engine referee. Usage (see `pt referee`):
#   godot --headless --path engine/godot --script referee.gd -- <model.glb> [nav=1]            import + counts + navmesh coverage
#   godot --path engine/godot --script referee.gd -- <model.glb> fps=1 [eye=x,y,z yaw=deg]     windowed: draw calls and frame rate
# Prints one line `REFEREE_JSON {...}`; exits 0 if the engine imported the file without errors, 1 otherwise.

const TERRAIN_NODE := "geometry"  # the base mesh of a Proteus export (instances are separate nodes)

var result := {"ok": false, "errors": [], "godot": Engine.get_version_info().string}


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
	_apply_lod(root)
	_count(root)
	if opts.has("nav"):
		await _navmesh(root, opts)
	if opts.has("fps"):
		await _fps(root, opts)
	result["ok"] = (result["errors"] as Array).is_empty()
	_finish("")


func _lod_level(n: Node) -> int:
	var s := String(n.name)
	var i := s.find("__lod")
	return int(s.substr(i + 5)) if i >= 0 else -1


# The engine bridge for LODs: the exporter writes one node per level with its distance range in the node extras
# (`proteus_lod_range`, metres); here they become visibility ranges, which is how an engine switches levels by distance.
func _apply_lod(root: Node) -> void:
	var applied := 0
	var stack: Array[Node] = [root]
	while not stack.is_empty():
		var n: Node = stack.pop_back()
		for c in n.get_children():
			stack.append(c)
		if n is GeometryInstance3D and n.has_meta("extras"):
			var ex: Variant = n.get_meta("extras")
			if ex is Dictionary and ex.has("proteus_lod_range"):
				var r: Array = ex["proteus_lod_range"]
				(n as GeometryInstance3D).visibility_range_begin = float(r[0])
				(n as GeometryInstance3D).visibility_range_end = float(r[1])
				(n as GeometryInstance3D).visibility_range_fade_mode = GeometryInstance3D.VISIBILITY_RANGE_FADE_DISABLED
				applied += 1
	result["lod_ranges_applied"] = applied


func _count(root: Node) -> void:
	var nodes := 0
	var mesh_instances := 0
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
	var unique_tris := 0
	for v in unique_meshes.values():
		unique_tris += v
	result["nodes"] = nodes
	result["mesh_instances"] = mesh_instances
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
	var terrain: MeshInstance3D = null
	for c in root.get_children():
		if c is MeshInstance3D and String(c.name).begins_with(TERRAIN_NODE):
			terrain = c
			break
	if terrain == null:
		(result["errors"] as Array).append("navmesh: no '%s' node (the base geometry) to bake" % TERRAIN_NODE)
		return
	var max_slope := float(opts.get("slope", "40"))
	var nav := NavigationMesh.new()
	nav.cell_size = float(opts.get("cell", "1.0"))
	nav.cell_height = 0.5
	nav.agent_radius = float(opts.get("radius", "1.0"))
	nav.agent_height = 1.8
	nav.agent_max_climb = 0.6
	nav.agent_max_slope = max_slope
	var src := NavigationMeshSourceGeometryData3D.new()
	src.add_mesh(terrain.mesh, terrain.global_transform)
	var t0 := Time.get_ticks_msec()
	NavigationServer3D.bake_from_source_geometry_data(nav, src)
	var bake_ms := Time.get_ticks_msec() - t0
	var verts := nav.get_vertices()
	var area := 0.0
	for p in nav.get_polygon_count():
		var poly := nav.get_polygon(p)
		for k in range(1, poly.size() - 1):
			var a := verts[poly[0]]
			var b := verts[poly[k]]
			var c := verts[poly[k + 1]]
			area += absf((b - a).cross(c - a).y) * 0.5  # horizontal area
	# expected: horizontal area of terrain triangles whose slope is within the limit, and the INTENDED walkable ground
	# (slope under `intended` degrees, 25 by default): the part of the world a designer means people to walk on
	var arrays := terrain.mesh.surface_get_arrays(0)
	var tv: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
	var ti: PackedInt32Array = arrays[Mesh.ARRAY_INDEX]
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
	for p in nav.get_polygon_count():
		var poly := nav.get_polygon(p)
		for k in range(1, poly.size() - 1):
			var tri := [verts[poly[0]], verts[poly[k]], verts[poly[k + 1]]]
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
		"polygons": nav.get_polygon_count(), "area_m2": snappedf(area, 1.0), "walkable_terrain_m2": snappedf(walkable, 1.0),
		"terrain_m2": snappedf(total, 1.0), "coverage": snappedf(area / maxf(walkable, 1.0), 0.001), "bake_ms": bake_ms,
		"max_slope_deg": max_slope, "agent_radius": nav.agent_radius,
		"intended_slope_deg": float(opts.get("intended", "25")), "intended_m2": snappedf(intended_area, 1.0),
		"coverage_intended": snappedf(float(reached) / maxf(samples, 1), 0.001), "samples": samples,
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
	for c in root.get_children():
		if c is MeshInstance3D and String(c.name).begins_with(TERRAIN_NODE) and c.mesh != null:
			var tm: TriangleMesh = (c as MeshInstance3D).mesh.generate_triangle_mesh()
			var hit := tm.intersect_ray(Vector3(x, 100000.0, z), Vector3(0, -1, 0))
			if not hit.is_empty():
				return float(hit["position"].y)
	return -1e9


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
	var cam := Camera3D.new()
	var lens := float(opts.get("lens", "28"))  # 36 mm sensor width, like the Blender render
	cam.fov = rad_to_deg(2.0 * atan((18.0 / lens) * (float(size.y) / float(size.x))))  # Godot's fov is vertical
	cam.far = 6000.0
	root.add_child(cam)
	cam.global_position = eye
	# Godot's camera looks down -Z; our yaw 0 looks along +X and grows toward +Z
	cam.rotation_degrees = Vector3(float(opts.get("pitch", "0")), -90.0 - float(opts.get("yaw", "0")), 0)
	cam.current = true
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-27, -60, 0)  # toward the sun at azimuth 150, elevation 27 (the alpine style)
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 400.0
	root.add_child(sun)
	var env := Environment.new()
	env.background_mode = Environment.BG_SKY
	env.sky = Sky.new()
	env.sky.sky_material = ProceduralSkyMaterial.new()
	env.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	var we := WorldEnvironment.new()
	we.environment = env
	root.add_child(we)
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


func _finish(error: String) -> void:
	if error != "":
		(result["errors"] as Array).append(error)
	print("REFEREE_JSON ", JSON.stringify(result))
	quit(0 if (result["errors"] as Array).is_empty() else 1)
