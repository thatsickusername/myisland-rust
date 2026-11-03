use tiny_skia::{Path, PathBuilder};

const K: f32 = 0.55228475;

/// Draws the exact island contour path with 10px symmetrical top-bezel ear flares
/// matching `MyIsland/index.html` (`islandPath`).
pub fn build_island_path(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let r = radius.min(width / 2.0).min(height).max(0.0);
    let ear = (10.0f32).min(height / 3.0).min(width / 2.0).max(0.0);

    pb.move_to(x - ear, y);
    pb.line_to(x + width + ear, y);
    pb.quad_to(x + width, y, x + width, y + ear);
    pb.line_to(x + width, y + height - r);

    // Bottom-right 90-degree corner arc
    pb.cubic_to(
        x + width,
        y + height - r * (1.0 - K),
        x + width - r * (1.0 - K),
        y + height,
        x + width - r,
        y + height,
    );

    pb.line_to(x + r, y + height);

    // Bottom-left 90-degree corner arc
    pb.cubic_to(
        x + r * (1.0 - K),
        y + height,
        x,
        y + height - r * (1.0 - K),
        x,
        y + height - r,
    );

    pb.line_to(x, y + ear);
    pb.quad_to(x, y, x - ear, y);
    pb.close();

    pb.finish()
}
