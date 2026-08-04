//! Camera. Pan, zoom, and the world<->screen conversions selection needs.
//!
//! Presentation-only state (§1.6). Where the camera is looking has no effect
//! on the simulation and is never hashed — two players watching the same
//! replay from different corners of the map must still agree bit for bit.

/// Tiles visible across the window height at zoom 1.
const BASE_TILES_ON_SCREEN: f64 = 32.0;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// World position at the centre of the view, in tiles.
    pub centre: [f64; 2],
    /// Larger is closer in.
    pub zoom: f64,
    pub viewport: [f32; 2],
}

impl Default for Camera {
    fn default() -> Self {
        Camera::new()
    }
}

impl Camera {
    pub const MIN_ZOOM: f64 = 0.25;
    pub const MAX_ZOOM: f64 = 8.0;

    pub const fn new() -> Camera {
        Camera {
            centre: [32.0, 32.0],
            zoom: 1.0,
            viewport: [1280.0, 720.0],
        }
    }

    /// Pixels per world tile at the current zoom.
    #[inline]
    pub fn pixels_per_tile(&self) -> f64 {
        (self.viewport[1] as f64) / (BASE_TILES_ON_SCREEN / self.zoom)
    }

    pub fn pan(&mut self, tiles_x: f64, tiles_y: f64) {
        self.centre[0] += tiles_x;
        self.centre[1] += tiles_y;
    }

    /// Multiplicative zoom, clamped.
    ///
    /// Multiplicative rather than additive so each notch of a scroll wheel
    /// feels the same at every distance — an additive step is imperceptible
    /// zoomed out and violent zoomed in.
    pub fn zoom_by(&mut self, factor: f64) {
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }

    /// World tiles -> normalised device coordinates.
    pub fn world_to_ndc(&self, world: [f64; 2]) -> [f32; 2] {
        let ppt = self.pixels_per_tile();
        let dx = (world[0] - self.centre[0]) * ppt;
        let dy = (world[1] - self.centre[1]) * ppt;
        [
            (dx / (self.viewport[0] as f64 / 2.0)) as f32,
            // Y is flipped: world Y grows downward (row order on the map),
            // NDC Y grows upward.
            (-dy / (self.viewport[1] as f64 / 2.0)) as f32,
        ]
    }

    /// Window pixels -> world tiles. This is what click-selection needs.
    pub fn screen_to_world(&self, screen: [f64; 2]) -> [f64; 2] {
        let ppt = self.pixels_per_tile();
        let dx = screen[0] - (self.viewport[0] as f64) / 2.0;
        let dy = screen[1] - (self.viewport[1] as f64) / 2.0;
        [self.centre[0] + dx / ppt, self.centre[1] + dy / ppt]
    }

    /// Scale factor to convert a world radius into NDC width.
    #[inline]
    pub fn tile_to_ndc_scale(&self) -> [f32; 2] {
        let ppt = self.pixels_per_tile();
        [
            (ppt / (self.viewport[0] as f64 / 2.0)) as f32,
            (ppt / (self.viewport[1] as f64 / 2.0)) as f32,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        let mut c = Camera::new();
        c.viewport = [800.0, 600.0];
        c
    }

    #[test]
    fn the_centre_maps_to_the_middle_of_the_screen() {
        let c = cam();
        let ndc = c.world_to_ndc(c.centre);
        assert!(ndc[0].abs() < 1e-6 && ndc[1].abs() < 1e-6, "{ndc:?}");
    }

    #[test]
    fn screen_to_world_inverts_world_to_ndc_at_the_centre() {
        let c = cam();
        let mid = [c.viewport[0] as f64 / 2.0, c.viewport[1] as f64 / 2.0];
        let w = c.screen_to_world(mid);
        assert!((w[0] - c.centre[0]).abs() < 1e-9);
        assert!((w[1] - c.centre[1]).abs() < 1e-9);
    }

    #[test]
    fn screen_to_world_round_trips_off_centre() {
        // The property click-selection depends on: what the player clicks is
        // what gets picked, at any zoom.
        for zoom in [0.5, 1.0, 3.0] {
            let mut c = cam();
            c.zoom = zoom;
            let world = [40.0, 25.0];
            let ppt = c.pixels_per_tile();
            let screen = [
                (world[0] - c.centre[0]) * ppt + c.viewport[0] as f64 / 2.0,
                (world[1] - c.centre[1]) * ppt + c.viewport[1] as f64 / 2.0,
            ];
            let back = c.screen_to_world(screen);
            assert!((back[0] - world[0]).abs() < 1e-9, "zoom {zoom}");
            assert!((back[1] - world[1]).abs() < 1e-9, "zoom {zoom}");
        }
    }

    #[test]
    fn world_y_grows_downward_on_screen() {
        // A unit south of the camera must draw below it, not above.
        let c = cam();
        let below = c.world_to_ndc([c.centre[0], c.centre[1] + 5.0]);
        assert!(
            below[1] < 0.0,
            "world +Y should be screen-down, got {below:?}"
        );
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let mut c = cam();
        for _ in 0..100 {
            c.zoom_by(2.0);
        }
        assert_eq!(c.zoom, Camera::MAX_ZOOM);
        for _ in 0..100 {
            c.zoom_by(0.5);
        }
        assert_eq!(c.zoom, Camera::MIN_ZOOM);
    }

    #[test]
    fn zoom_is_multiplicative_so_every_notch_feels_the_same() {
        let mut a = cam();
        a.zoom = 1.0;
        a.zoom_by(1.1);
        let step_near = a.zoom - 1.0;
        let mut b = cam();
        b.zoom = 4.0;
        b.zoom_by(1.1);
        let step_far = b.zoom - 4.0;
        assert!(
            step_far > step_near,
            "zoom step did not scale with distance"
        );
    }

    #[test]
    fn panning_moves_the_view_not_the_world() {
        let mut c = cam();
        let before = c.world_to_ndc([50.0, 50.0]);
        c.pan(10.0, 0.0);
        let after = c.world_to_ndc([50.0, 50.0]);
        assert!(
            after[0] < before[0],
            "panning right should move things left"
        );
    }
}
