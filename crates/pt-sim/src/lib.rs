//! Deterministic simulation solvers: particles (POP), position-based cloth, smoke, and
//! (feature-gated) rigid bodies. All solvers are pure functions of (inputs, params, frame).

pub mod cloth;
pub mod pop;
pub mod rbd;
pub mod smoke;
