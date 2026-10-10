//! Tiny 3D helpers for the wireframe loops: rotate, project, and shade by depth.

#[derive(Clone, Copy, Debug)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl V3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn rot_x(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }

    pub fn rot_y(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }

    pub fn rot_z(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c, self.z)
    }

    /// Orthographic projection: scale by `s` about screen point `c`.
    pub fn screen(self, c: (f32, f32), s: f32) -> (f32, f32) {
        (c.0 + self.x * s, c.1 + self.y * s)
    }
}

/// Brightness for depth `z` in -1 (far) ..= 1 (near), blending `far` to `near`.
pub fn shade(z: f32, far: f32, near: f32) -> u8 {
    let k = ((z + 1.0) / 2.0).clamp(0.0, 1.0);
    (far + (near - far) * k) as u8
}

/// Center of `focus` and the scale unit for centered compositions.
pub fn center(focus: super::Rect) -> ((f32, f32), f32) {
    (
        (focus.x + focus.w / 2.0, focus.y + focus.h / 2.0),
        focus.unit(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_preserve_length_and_shade_is_monotonic() {
        let p = V3::new(0.3, -0.4, 0.5).rot_x(1.1).rot_y(-0.7);
        assert!(((p.x * p.x + p.y * p.y + p.z * p.z).sqrt() - 0.5f32.sqrt()).abs() < 1e-5);
        let q = V3::new(1.0, 0.0, 0.0).rot_y(std::f32::consts::FRAC_PI_2);
        assert!(q.x.abs() < 1e-6 && (q.z + 1.0).abs() < 1e-6);
        assert!(shade(-1.0, 40.0, 200.0) < shade(0.0, 40.0, 200.0));
        assert_eq!(shade(1.0, 40.0, 200.0), 200);
    }
}
