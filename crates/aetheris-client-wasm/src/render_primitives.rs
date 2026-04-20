use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable, Serialize, Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
}

/// Generates a sharp pyramid-like ship for the Interceptor.
pub fn create_interceptor_mesh() -> MeshData {
    let mut vertices = Vec::new();

    // Helper to add a triangle with its own surface normal
    let mut add_face = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]| {
        let v0 = glam::Vec3::from_array(p0);
        let v1 = glam::Vec3::from_array(p1);
        let v2 = glam::Vec3::from_array(p2);
        let normal = (v1 - v0).cross(v2 - v0).normalize().to_array();

        vertices.push(Vertex {
            position: p0,
            normal,
        });
        vertices.push(Vertex {
            position: p1,
            normal,
        });
        vertices.push(Vertex {
            position: p2,
            normal,
        });
    };

    let nose = [0.0, 0.0, 1.0];
    let back_left = [-0.4, -0.2, -0.5];
    let back_right = [0.4, -0.2, -0.5];
    let top = [0.0, 0.3, -0.3];

    add_face(nose, back_left, top); // Left face
    add_face(nose, top, back_right); // Right face
    add_face(nose, back_right, back_left); // Bottom face
    add_face(back_left, back_right, top); // Back face

    let indices = (0..vertices.len() as u16).collect();

    MeshData { vertices, indices }
}

/// Generates a bulky blocky ship for the Dreadnought.
pub fn create_dreadnought_mesh() -> MeshData {
    // Bulky cube-shaped mesh (width w, height h, length l)
    let h = 0.5;
    let w = 0.8;
    let l = 1.5;

    create_cube_mesh(w, h, l)
}

pub fn create_cube_mesh(w: f32, h: f32, l: f32) -> MeshData {
    let vertices = vec![
        // Top
        Vertex {
            position: [-w, h, l],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [w, h, l],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [w, h, -l],
            normal: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [-w, h, -l],
            normal: [0.0, 1.0, 0.0],
        },
        // Bottom
        Vertex {
            position: [-w, -h, l],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [w, -h, l],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [w, -h, -l],
            normal: [0.0, -1.0, 0.0],
        },
        Vertex {
            position: [-w, -h, -l],
            normal: [0.0, -1.0, 0.0],
        },
        // Front
        Vertex {
            position: [-w, -h, l],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [w, -h, l],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [w, h, l],
            normal: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [-w, h, l],
            normal: [0.0, 0.0, 1.0],
        },
        // Back
        Vertex {
            position: [-w, -h, -l],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [w, -h, -l],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [w, h, -l],
            normal: [0.0, 0.0, -1.0],
        },
        Vertex {
            position: [-w, h, -l],
            normal: [0.0, 0.0, -1.0],
        },
        // Left
        Vertex {
            position: [-w, -h, -l],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [-w, -h, l],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [-w, h, l],
            normal: [-1.0, 0.0, 0.0],
        },
        Vertex {
            position: [-w, h, -l],
            normal: [-1.0, 0.0, 0.0],
        },
        // Right
        Vertex {
            position: [w, -h, -l],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [w, -h, l],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [w, h, l],
            normal: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [w, h, -l],
            normal: [1.0, 0.0, 0.0],
        },
    ];

    let mut indices = Vec::new();
    for i in 0..6 {
        let base = (i * 4) as u16;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    MeshData { vertices, indices }
}

pub fn create_asteroid_mesh() -> MeshData {
    // Low-poly octahedron with per-face normals computed via cross product.
    let mut vertices = Vec::new();

    let mut add_face = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]| {
        let v0 = glam::Vec3::from_array(p0);
        let v1 = glam::Vec3::from_array(p1);
        let v2 = glam::Vec3::from_array(p2);
        let normal = (v1 - v0).cross(v2 - v0).normalize().to_array();
        vertices.push(Vertex {
            position: p0,
            normal,
        });
        vertices.push(Vertex {
            position: p1,
            normal,
        });
        vertices.push(Vertex {
            position: p2,
            normal,
        });
    };

    // Top apex
    let top = [0.0, 1.0, 0.0];
    let bot = [0.0, -1.0, 0.0];
    let right = [1.0, 0.0, 0.0];
    let left = [-1.0, 0.0, 0.0];
    let front = [0.0, 0.0, 1.0];
    let back = [0.0, 0.0, -1.0];

    // Upper hemisphere
    add_face(top, right, front);
    add_face(top, front, left);
    add_face(top, left, back);
    add_face(top, back, right);
    // Lower hemisphere
    add_face(bot, front, right);
    add_face(bot, left, front);
    add_face(bot, back, left);
    add_face(bot, right, back);

    let indices = (0..vertices.len() as u16).collect();
    MeshData { vertices, indices }
}

pub fn create_projectile_mesh() -> MeshData {
    // Small diamond (bipyramid) with per-face normals computed via cross product.
    let mut vertices = Vec::new();

    let mut add_face = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3]| {
        let v0 = glam::Vec3::from_array(p0);
        let v1 = glam::Vec3::from_array(p1);
        let v2 = glam::Vec3::from_array(p2);
        let normal = (v1 - v0).cross(v2 - v0).normalize().to_array();
        vertices.push(Vertex {
            position: p0,
            normal,
        });
        vertices.push(Vertex {
            position: p1,
            normal,
        });
        vertices.push(Vertex {
            position: p2,
            normal,
        });
    };

    let s = 0.1;
    let l = 0.4;
    let front = [0.0, 0.0, l];
    let back = [0.0, 0.0, -l];
    let right = [s, 0.0, 0.0];
    let left = [-s, 0.0, 0.0];
    let up = [0.0, s, 0.0];
    let down = [0.0, -s, 0.0];

    // Front cone
    add_face(front, right, up);
    add_face(front, up, left);
    add_face(front, left, down);
    add_face(front, down, right);
    // Back cone
    add_face(back, up, right);
    add_face(back, left, up);
    add_face(back, down, left);
    add_face(back, right, down);

    let indices = (0..vertices.len() as u16).collect();
    MeshData { vertices, indices }
}
