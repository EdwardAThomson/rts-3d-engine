//! Just enough vector and matrix maths for a camera. Matrices are column-major, as WGSL lays out `mat4x4<f32>`.

pub type V3 = [f32; 3];

/// A 4 by 4 matrix as four columns.
pub type Mat4 = [[f32; 4]; 4];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

pub fn normalize(a: V3) -> V3 {
    scale(a, 1.0 / length(a))
}

/// `a * b`: applies `b` first.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, cell) in column.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// `m * (p, 1)`.
pub fn transform(m: &Mat4, p: V3) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (r, cell) in out.iter_mut().enumerate() {
        *cell = m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r];
    }
    out
}
