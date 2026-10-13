extends Node
# The walk-through (`pt walk`): move through the world in the engine and point at what looks wrong.
#
# Controls: hold the right mouse button to look around; W A S D to move, Q and E down and up, Shift faster, the mouse wheel
# changes the speed; G walks on the ground or flies; 1 to 9 jump to the scene's cameras; F shows the flagged objects.
# A left click on anything notes it ("this looks wrong"), X notes "remove this": each note is one JSON line in the plan's
# `notes` file with the ray through the cursor and a picture of what was on screen, and `pt walk` names the object afterwards.

signal closed

const EYE_HEIGHT := 1.7
const HELP := "Hold the right mouse button: look around      W A S D: move      Q / E: down / up      Shift: faster      Mouse wheel: speed\nG: walk on the ground / fly      1-9: jump to a camera      F: show flagged objects      H: hide this help      Esc: close\nLeft click on anything that looks wrong: it is noted      X: note \"remove this\" for what is under the cursor"

var cam: Camera3D
var plan := {}
var bases: Array = []
var show_flags: Callable
var yaw := 0.0  # degrees: 0 looks along +X and grows toward +Z, like the scene file
var pitch := 0.0
var speed := 6.0
var looking := false
var on_ground := false
var flags_on := false
var notes := 0
var ground: Array = []  # [TriangleMesh, AABB] of every terrain surface, built the first time G is pressed
var layer: CanvasLayer
var status: Label
var help: Label
var toast := ""
var toast_until := 0


func setup(camera: Camera3D, the_plan: Dictionary, terrain_nodes: Array, flags: Callable) -> void:
	cam = camera
	plan = the_plan
	bases = terrain_nodes
	show_flags = flags


func _ready() -> void:
	layer = CanvasLayer.new()
	add_child(layer)
	status = _label(Vector2(12, 8))
	help = _label(Vector2(12, 40))
	help.text = HELP
	var cams: Array = plan.get("cameras", [])
	if not cams.is_empty():
		_jump(cams[0])
	_place()
	if bool(plan.get("check", false)):
		_check()


# `pt walk --check`: nobody is at the keyboard. Note what is in the middle of the first camera's view, the way a click does,
# and close: the whole path (window, controls' note, picture, naming) is proven on this machine in a few seconds.
func _check() -> void:
	for i in 30:
		await get_tree().process_frame
	await _note("flag", get_viewport().get_visible_rect().size / 2.0)
	for i in 10:
		await get_tree().process_frame
	closed.emit()


func _label(at: Vector2) -> Label:
	var l := Label.new()
	l.position = at
	l.add_theme_color_override("font_color", Color(1, 1, 1))
	l.add_theme_color_override("font_outline_color", Color(0, 0, 0))
	l.add_theme_constant_override("outline_size", 6)
	layer.add_child(l)
	return l


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		closed.emit()


func _jump(c: Dictionary) -> void:
	var e: Array = c["eye"]
	cam.global_position = Vector3(e[0], e[1], e[2])
	yaw = float(c.get("yaw", 0.0))
	pitch = float(c.get("pitch", 0.0))
	# the camera's own lens (36 mm sensor width; Godot's field of view is vertical)
	var size := get_viewport().get_visible_rect().size
	cam.fov = rad_to_deg(2.0 * atan((18.0 / float(c.get("lens", 28.0))) * (size.y / maxf(size.x, 1.0))))
	_say("camera %s" % String(c.get("name", "?")))


func _place() -> void:
	# Godot's camera looks down -Z; our yaw 0 looks along +X and grows toward +Z
	cam.rotation_degrees = Vector3(pitch, -90.0 - yaw, 0)


func _say(text: String) -> void:
	toast = text
	toast_until = Time.get_ticks_msec() + 2500


func _input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var b := event as InputEventMouseButton
		if b.button_index == MOUSE_BUTTON_RIGHT:
			looking = b.pressed
			Input.mouse_mode = Input.MOUSE_MODE_CAPTURED if looking else Input.MOUSE_MODE_VISIBLE
		elif b.button_index == MOUSE_BUTTON_LEFT and b.pressed and not looking:
			_note("flag", b.position)
		elif b.button_index == MOUSE_BUTTON_WHEEL_UP and b.pressed:
			speed = minf(speed * 1.25, 400.0)
		elif b.button_index == MOUSE_BUTTON_WHEEL_DOWN and b.pressed:
			speed = maxf(speed / 1.25, 0.5)
	elif event is InputEventMouseMotion and looking:
		var m := event as InputEventMouseMotion
		yaw += m.relative.x * 0.15
		pitch = clampf(pitch - m.relative.y * 0.15, -89.0, 89.0)
		_place()
	elif event is InputEventKey and (event as InputEventKey).pressed and not (event as InputEventKey).echo:
		var k := (event as InputEventKey).keycode
		if k == KEY_ESCAPE:
			closed.emit()
		elif k == KEY_H:
			help.visible = not help.visible
		elif k == KEY_F:
			flags_on = not flags_on
			show_flags.call(flags_on)
			_say("flagged objects: %s" % ("shown (red floats, orange hangs, yellow is mostly under water, magenta is buried)" if flags_on else "hidden"))
		elif k == KEY_G:
			on_ground = not on_ground
			if on_ground and ground.is_empty():
				for n in bases:
					var mi := n as MeshInstance3D
					ground.append([mi.mesh.generate_triangle_mesh(), mi.get_aabb()])
			_say("walking on the ground" if on_ground else "flying")
		elif k == KEY_X:
			_note("remove", get_viewport().get_mouse_position())
		elif k >= KEY_1 and k <= KEY_9:
			var cams: Array = plan.get("cameras", [])
			if k - KEY_1 < cams.size():
				_jump(cams[k - KEY_1])
				_place()


func _ground_y(x: float, z: float) -> float:
	var best := -1e9
	for g in ground:
		var box: AABB = g[1]
		if x < box.position.x or x > box.end.x or z < box.position.z or z > box.end.z:
			continue
		var hit: Dictionary = (g[0] as TriangleMesh).intersect_ray(Vector3(x, 100000.0, z), Vector3(0, -1, 0))
		if not hit.is_empty():
			best = maxf(best, float(hit["position"].y))
	return best


func _process(delta: float) -> void:
	var y := deg_to_rad(yaw)
	var p := deg_to_rad(0.0 if on_ground else pitch)
	var fwd := Vector3(cos(y) * cos(p), sin(p), sin(y) * cos(p))
	var right := Vector3(-sin(y), 0, cos(y))
	var move := Vector3.ZERO
	if Input.is_key_pressed(KEY_W):
		move += fwd
	if Input.is_key_pressed(KEY_S):
		move -= fwd
	if Input.is_key_pressed(KEY_D):
		move += right
	if Input.is_key_pressed(KEY_A):
		move -= right
	if not on_ground:
		if Input.is_key_pressed(KEY_E):
			move += Vector3.UP
		if Input.is_key_pressed(KEY_Q):
			move -= Vector3.UP
	var pos := cam.global_position
	if move != Vector3.ZERO:
		pos += move.normalized() * speed * (4.0 if Input.is_key_pressed(KEY_SHIFT) else 1.0) * delta
	if on_ground:
		var gy := _ground_y(pos.x, pos.z)
		if gy > -1e8:
			pos.y = gy + EYE_HEIGHT
	cam.global_position = pos
	var line := "at %.1f, %.1f, %.1f   looking %.0f / %.0f   speed %.1f m/s   %s   notes: %d" % [pos.x, pos.y, pos.z, yaw, pitch, speed, "walking" if on_ground else "flying", notes]
	if Time.get_ticks_msec() < toast_until:
		line += "      " + toast
	status.text = line


# One note: what the owner pointed at. The ray through the cursor is written down (the build casts it into the cooked world and
# names the object), with a picture of the screen without the help text.
func _note(what: String, at: Vector2) -> void:
	notes += 1
	var origin := cam.project_ray_origin(at)
	var dir := cam.project_ray_normal(at)
	var size := get_viewport().get_visible_rect().size
	var shot := "%s/note_%03d.png" % [String(plan.get("shots", ".")), notes]
	var pos := cam.global_position
	var rec := {
		"n": notes, "what": what, "origin": [origin.x, origin.y, origin.z], "dir": [dir.x, dir.y, dir.z], "pixel": [at.x, at.y],
		"size": [size.x, size.y], "eye": [pos.x, pos.y, pos.z], "yaw": yaw, "pitch": pitch, "shot": shot,
	}
	var path := String(plan.get("notes", "notes.jsonl"))
	var f := FileAccess.open(path, FileAccess.READ_WRITE) if FileAccess.file_exists(path) else FileAccess.open(path, FileAccess.WRITE)
	if f != null:
		f.seek_end()
		f.store_line(JSON.stringify(rec))
		f.close()
	layer.visible = false
	await RenderingServer.frame_post_draw
	var img := get_viewport().get_texture().get_image()
	if img != null:
		img.save_png(shot)
	layer.visible = true
	_say("noted %d: %s" % [notes, "remove this" if what == "remove" else "looks wrong"])
