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

/// Maximum gap (px) at which the viscous liquid neck can still bridge the two pills.
pub const LIQUID_BRIDGE_MAX_GAP: f32 = 38.0;

/// Builds the organic liquid bridge between the primary island and secondary bubble.
///
/// Instead of a thick hanging blob, the bridge follows the existing top-bezel
/// ear curves of both pills: the main island's ear flows into the seam, the
/// seam narrows to a thin concave waist in the middle, and then flows back out
/// along the secondary pill's ear. Endpoint control points are shared with the
/// ear curves, so the union is G1-continuous (no visible corners).
pub fn build_liquid_bridge_path(
    island_x: f32,
    island_w: f32,
    sec_x: f32,
    h: f32,
    sec_h: f32,
    gap: f32,
) -> Option<Path> {
    if gap <= -2.0 || gap >= LIQUID_BRIDGE_MAX_GAP || h <= 10.0 || sec_h <= 10.0 {
        return None;
    }

    let left = island_x + island_w;
    let right = sec_x;

    // Match `build_island_path`'s ear flare exactly so the seam is continuous.
    let ear = 10.0f32.min(h / 3.0).min(sec_h / 3.0).max(0.0);
    let left_tip = left + ear;
    let right_tip = right - ear;

    // If the ear flares already touch/overlap, the union is connected without
    // an explicit bridge (and a reversed top edge would self-intersect).
    if right_tip <= left_tip + 0.5 {
        return None;
    }

    let mut pb = PathBuilder::new();

    // Start at the main island's right-ear base, climb its ear to the top edge.
    pb.move_to(left, ear);
    pb.quad_to(left, 0.0, left_tip, 0.0);

    // Flat top seam across the gap between the two ear tips.
    pb.line_to(right_tip, 0.0);

    // Descend along the secondary pill's left-ear curve.
    pb.quad_to(right, 0.0, right, ear);

    // Concave waist: control points sit on the top edge at the pill walls, so
    // the seam leaves each ear with a vertical tangent and pinches to a thin
    // ~ear/4 film in the middle (a liquid neck rather than a filled blob).
    pb.cubic_to(right, 0.0, left, 0.0, left, ear);

    pb.close();

    pb.finish()
}
