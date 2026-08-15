use std::collections::HashMap;

const PINCH_SENSITIVITY: f32 = 18.0;

/// Converts an incremental pinch scale into the same units used by a mouse
/// wheel. SDL defines values above one as zooming in and below one as zooming
/// out, which matches `Camera::zoom`.
pub fn pinch_scale_to_zoom_delta(scale: f32) -> Option<f32> {
    (scale.is_finite() && scale > 0.0).then(|| (scale.ln() * PINCH_SENSITIVITY).clamp(-4.0, 4.0))
}

/// Touchscreen fallback for platforms that emit finger motion but no native
/// SDL pinch events. Positions are normalized window coordinates.
#[derive(Default)]
pub struct TouchZoom {
    fingers: HashMap<(u64, u64), [f32; 2]>,
}

impl TouchZoom {
    pub fn clear(&mut self) {
        self.fingers.clear();
    }

    pub fn finger_down(&mut self, touch_id: u64, finger_id: u64, x: f32, y: f32) {
        self.fingers.insert((touch_id, finger_id), [x, y]);
    }

    pub fn finger_up(&mut self, touch_id: u64, finger_id: u64) {
        self.fingers.remove(&(touch_id, finger_id));
    }

    pub fn finger_motion(&mut self, touch_id: u64, finger_id: u64, x: f32, y: f32) -> Option<f32> {
        let old_span = self.two_finger_span(touch_id);
        self.fingers.insert((touch_id, finger_id), [x, y]);
        let new_span = self.two_finger_span(touch_id);
        match (old_span, new_span) {
            (Some(old), Some(new)) if old > f32::EPSILON => pinch_scale_to_zoom_delta(new / old),
            _ => None,
        }
    }

    fn two_finger_span(&self, touch_id: u64) -> Option<f32> {
        let points: Vec<_> = self
            .fingers
            .iter()
            .filter_map(|(&(candidate_touch, _), &point)| {
                (candidate_touch == touch_id).then_some(point)
            })
            .collect();
        if points.len() != 2 {
            return None;
        }
        let dx = points[0][0] - points[1][0];
        let dy = points[0][1] - points[1][1];
        Some(dx.hypot(dy))
    }
}

#[cfg(test)]
mod tests {
    use super::{TouchZoom, pinch_scale_to_zoom_delta};

    #[test]
    fn native_pinch_direction_matches_camera_zoom() {
        assert!(pinch_scale_to_zoom_delta(1.1).unwrap() > 0.0);
        assert!(pinch_scale_to_zoom_delta(0.9).unwrap() < 0.0);
        assert_eq!(pinch_scale_to_zoom_delta(1.0), Some(0.0));
        assert_eq!(pinch_scale_to_zoom_delta(0.0), None);
    }

    #[test]
    fn two_finger_spread_zooms_in_and_pinch_zooms_out() {
        let mut touch = TouchZoom::default();
        touch.finger_down(1, 10, 0.4, 0.5);
        touch.finger_down(1, 11, 0.6, 0.5);
        assert!(touch.finger_motion(1, 11, 0.7, 0.5).unwrap() > 0.0);
        assert!(touch.finger_motion(1, 11, 0.5, 0.5).unwrap() < 0.0);
    }

    #[test]
    fn a_third_finger_suspends_pinch_tracking() {
        let mut touch = TouchZoom::default();
        touch.finger_down(1, 10, 0.4, 0.5);
        touch.finger_down(1, 11, 0.6, 0.5);
        touch.finger_down(1, 12, 0.8, 0.5);
        assert_eq!(touch.finger_motion(1, 11, 0.7, 0.5), None);
    }
}
