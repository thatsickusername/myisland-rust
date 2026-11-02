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

/// Builds the organic liquid bridge between the primary island and secondary bubble.
pub fn build_liquid_bridge_path(
    island_x: f32,
    island_w: f32,
    sec_x: f32,
    h: f32,
    sec_h: f32,
    gap: f32,
) -> Option<Path> {
    if gap <= -2.0 || gap >= 22.0 || h <= 10.0 || sec_h <= 10.0 {
        return None;
    }

    let mut pb = PathBuilder::new();
    let bridge_top = 0.0;
    let bridge_bottom = h.min(sec_h) * 0.75;
    let mid_x = (island_x + island_w + sec_x) / 2.0;
    let dip_y = bridge_bottom * (1.0 - gap.max(0.0) / 22.0) * 0.6;

    pb.move_to(island_x + island_w - 2.0, bridge_top);
    pb.line_to(sec_x + 2.0, bridge_top);
    pb.line_to(sec_x + 2.0, bridge_bottom);
    pb.quad_to(mid_x, bridge_bottom - dip_y, island_x + island_w - 2.0, bridge_bottom);
    pb.close();

    pb.finish()
}
