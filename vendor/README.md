# Vendored, patched dependencies

Copies of `rapier3d 0.36.0`, `parry3d 0.31.1` and `glamx 0.3.1` (Apache-2.0, original licenses kept in each
folder). They are wired in through `[patch.crates-io]` in the root `Cargo.toml`.

**Why:** rigid-body results differed between Windows and Linux/macOS. `pt trace` showed every upstream node was
bit-identical and only `rbd_sim` diverged: the physics stack calls `f32::acos`, `cos`, `sin`, `log2` from the
operating system's C library (in the inertia eigen-solver, BVH optimizer, convex-polyhedron feature selection),
and those round differently per OS even with rapier's `enhanced-determinism`.

**Patch:** each such call on the 3D simulation path now calls the pure-software `libm` crate (`detlibm::acosf`
etc.; `glamx::detmath` for the generic eigen-solver). Search for `detlibm` / `detmath` to see every change.
Calls inside generic `RealField` code already use `libm` through simba's `libm_force`.

**Upgrading:** re-apply the patches (grep above) to the new upstream version and check `pt recipes` plus the
cross-OS CI job; `pt trace <recipe>` finds the first diverging node.
