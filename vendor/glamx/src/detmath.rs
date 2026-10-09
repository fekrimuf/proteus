//! Platform-independent transcendental functions (software `libm`), used wherever results must be
//! bit-identical across operating systems. Patched into this vendored copy by Proteus.

pub trait DetFloat: Sized {
    fn d_acos(self) -> Self;
    fn d_cos(self) -> Self;
    fn d_sin(self) -> Self;
}

impl DetFloat for f32 {
    fn d_acos(self) -> f32 {
        detlibm::acosf(self)
    }
    fn d_cos(self) -> f32 {
        detlibm::cosf(self)
    }
    fn d_sin(self) -> f32 {
        detlibm::sinf(self)
    }
}

impl DetFloat for f64 {
    fn d_acos(self) -> f64 {
        detlibm::acos(self)
    }
    fn d_cos(self) -> f64 {
        detlibm::cos(self)
    }
    fn d_sin(self) -> f64 {
        detlibm::sin(self)
    }
}
