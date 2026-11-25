#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Capture,
    SwitchCamera,
    Flash,
    Raw,
    ExposureDown,
    ExposureUp,
}

fn inside(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32) -> bool {
    x >= left && x <= right && y >= top && y <= bottom
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> bool {
    (x - cx).powi(2) + (y - cy).powi(2) <= radius.powi(2)
}

/// Hit test in window pixel coordinates (origin top-left).
pub fn hit_test(width: i32, height: i32, x: f32, y: f32) -> Option<Control> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let w = width as f32;
    let h = height as f32;
    let short = w.min(h);
    let button_radius = (short * 0.065).max(28.0);

    if circle(x, y, w * 0.5, h * 0.88, button_radius * 1.35) {
        return Some(Control::Capture);
    }
    if circle(x, y, w * 0.86, h * 0.86, button_radius) {
        return Some(Control::SwitchCamera);
    }
    if circle(x, y, w * 0.14, h * 0.86, button_radius) {
        return Some(Control::Flash);
    }
    if inside(x, y, w * 0.80, h * 0.03, w * 0.98, h * 0.12) {
        return Some(Control::Raw);
    }
    if inside(x, y, w * 0.03, h * 0.03, w * 0.15, h * 0.12) {
        return Some(Control::ExposureDown);
    }
    if inside(x, y, w * 0.17, h * 0.03, w * 0.29, h * 0.12) {
        return Some(Control::ExposureUp);
    }
    None
}
