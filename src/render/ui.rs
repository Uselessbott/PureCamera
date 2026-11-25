//! Minimal dark controls drawn with GLES primitives (no text renderer needed).

use super::gl::{self, Program};
use crate::camera::FlashMode;
use crate::ui::UiState;

const VERTEX: &str = "
attribute vec2 aPosition;
void main() { gl_Position = vec4(aPosition, 0.0, 1.0); }
";

const FRAGMENT: &str = "
precision mediump float;
uniform vec4 uColor;
void main() { gl_FragColor = uColor; }
";

pub struct UiRenderer {
    program: Program,
    buffer: u32,
    position: i32,
    color: i32,
}

impl UiRenderer {
    pub fn new() -> Result<Self, String> {
        let program = Program::new(VERTEX, FRAGMENT)?;
        let position = program.attribute("aPosition");
        let color = program.uniform("uColor");
        let mut buffer = 0;
        unsafe { gl::glGenBuffers(1, &mut buffer) };
        Ok(Self { program, buffer, position, color })
    }

    pub fn draw(&self, width: i32, height: i32, state: &UiState) {
        if width <= 0 || height <= 0 {
            return;
        }
        self.program.use_program();
        unsafe {
            gl::glEnable(gl::GL_BLEND);
            gl::glBlendFunc(gl::GL_SRC_ALPHA, gl::GL_ONE_MINUS_SRC_ALPHA);
            gl::glBindBuffer(gl::GL_ARRAY_BUFFER, self.buffer);
            if self.position >= 0 {
                gl::glEnableVertexAttribArray(self.position as u32);
                gl::glVertexAttribPointer(
                    self.position as u32,
                    2,
                    gl::GL_FLOAT,
                    0,
                    0,
                    std::ptr::null(),
                );
            }
        }

        let w = width as f32;
        let h = height as f32;
        let short = w.min(h);
        let button_radius = (short * 0.065).max(28.0);

        // Subtle dark bars keep controls legible without a full menu.
        self.rect(0.0, 0.0, w, h * 0.11, [0.0, 0.0, 0.0, 0.45]);
        self.rect(0.0, h * 0.76, w, h * 0.24, [0.0, 0.0, 0.0, 0.45]);

        // Shutter: white ring, dark centre, small white core.
        self.circle(w * 0.5, h * 0.88, button_radius * 1.35, [1.0, 1.0, 1.0, 0.9]);
        self.circle(w * 0.5, h * 0.88, button_radius, [0.12, 0.12, 0.12, 1.0]);
        self.circle(w * 0.5, h * 0.88, button_radius * 0.72, [0.95, 0.95, 0.95, 1.0]);

        self.draw_flash(w, h, button_radius, state.flash);
        self.draw_switch(w, h, button_radius);
        self.draw_raw(w, h, state);
        self.draw_exposure(w, h);

        unsafe { gl::glDisable(gl::GL_BLEND) };
    }

    fn draw_flash(&self, w: f32, h: f32, radius: f32, mode: FlashMode) {
        let cx = w * 0.14;
        let cy = h * 0.86;
        self.circle(cx, cy, radius, [0.12, 0.12, 0.12, 0.85]);
        let white = [1.0, 1.0, 1.0, 1.0];
        match mode {
            FlashMode::Auto => {
                // Three strokes form a small "A".
                self.line(cx - radius * 0.42, cy + radius * 0.40, cx, cy - radius * 0.42, 4.0, white);
                self.line(cx, cy - radius * 0.42, cx + radius * 0.42, cy + radius * 0.40, 4.0, white);
                self.line(cx - radius * 0.22, cy + radius * 0.10, cx + radius * 0.22, cy + radius * 0.10, 3.0, white);
            }
            FlashMode::On => {
                self.circle(cx, cy, radius * 0.42, white);
            }
            FlashMode::Off => {
                self.line(cx - radius * 0.38, cy - radius * 0.38, cx + radius * 0.38, cy + radius * 0.38, 5.0, white);
                self.line(cx - radius * 0.38, cy + radius * 0.38, cx + radius * 0.38, cy - radius * 0.38, 5.0, white);
            }
        }
    }

    fn draw_switch(&self, w: f32, h: f32, radius: f32) {
        let cx = w * 0.86;
        let cy = h * 0.86;
        self.circle(cx, cy, radius, [0.12, 0.12, 0.12, 0.85]);
        let white = [1.0, 1.0, 1.0, 1.0];
        self.line(cx - radius * 0.45, cy - radius * 0.20, cx + radius * 0.45, cy - radius * 0.20, 4.0, white);
        self.triangle(cx - radius * 0.45, cy - radius * 0.20, cx - radius * 0.15, cy - radius * 0.42, cx - radius * 0.15, cy + radius * 0.02, white);
        self.line(cx + radius * 0.45, cy + radius * 0.20, cx - radius * 0.45, cy + radius * 0.20, 4.0, white);
        self.triangle(cx + radius * 0.45, cy + radius * 0.20, cx + radius * 0.15, cy - radius * 0.02, cx + radius * 0.15, cy + radius * 0.42, white);
    }

    fn draw_raw(&self, w: f32, h: f32, state: &UiState) {
        let left = w * 0.80;
        let top = h * 0.03;
        let width = w * 0.18;
        let height = h * 0.09;
        self.rect(left, top, width, height, [0.0, 0.0, 0.0, 0.55]);
        let inset = height * 0.25;
        let colour = if state.raw_supported && state.raw_mode {
            [1.0, 1.0, 1.0, 1.0]
        } else if state.raw_supported {
            [1.0, 1.0, 1.0, 0.45]
        } else {
            [1.0, 0.35, 0.35, 0.65]
        };
        self.rect(left + inset, top + inset, width - inset * 2.0, height - inset * 2.0, colour);
    }

    fn draw_exposure(&self, w: f32, h: f32) {
        let top = h * 0.03;
        let height = h * 0.09;
        let white = [1.0, 1.0, 1.0, 0.9];
        let minus_left = w * 0.03;
        let plus_left = w * 0.17;
        let width = w * 0.12;
        self.rect(minus_left, top, width, height, [0.0, 0.0, 0.0, 0.55]);
        self.rect(minus_left + width * 0.25, top + height * 0.46, width * 0.5, height * 0.08, white);
        self.rect(plus_left, top, width, height, [0.0, 0.0, 0.0, 0.55]);
        self.rect(plus_left + width * 0.25, top + height * 0.46, width * 0.5, height * 0.08, white);
        self.rect(plus_left + width * 0.46, top + height * 0.25, width * 0.08, height * 0.5, white);
    }

    fn rect(&self, x: f32, y: f32, width: f32, height: f32, colour: [f32; 4]) {
        let x0 = ndc_x(x);
        let y0 = ndc_y(y + height);
        let x1 = ndc_x(x + width);
        let y1 = ndc_y(y);
        self.upload(&[x0, y0, x1, y0, x0, y1, x1, y1]);
        self.draw(gl::GL_TRIANGLE_STRIP, 4, colour);
    }

    fn circle(&self, cx: f32, cy: f32, radius: f32, colour: [f32; 4]) {
        let segments = 24;
        let mut vertices = Vec::with_capacity((segments + 2) * 2);
        vertices.push(ndc_x(cx));
        vertices.push(ndc_y(cy));
        for index in 0..=segments {
            let angle = index as f32 * std::f32::consts::TAU / segments as f32;
            vertices.push(ndc_x(cx + radius * angle.cos()));
            vertices.push(ndc_y(cy + radius * angle.sin()));
        }
        self.upload(&vertices);
        self.draw(gl::GL_TRIANGLE_FAN, (segments + 2) as i32, colour);
    }

    fn triangle(&self, x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32, colour: [f32; 4]) {
        self.upload(&[ndc_x(x0), ndc_y(y0), ndc_x(x1), ndc_y(y1), ndc_x(x2), ndc_y(y2)]);
        self.draw(gl::GL_TRIANGLES, 3, colour);
    }

    fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, thickness: f32, colour: [f32; 4]) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let length = dx.hypot(dy);
        if length <= 0.0 {
            return;
        }
        let nx = -dy / length * thickness * 0.5;
        let ny = dx / length * thickness * 0.5;
        self.upload(&[
            ndc_x(x0 + nx), ndc_y(y0 + ny),
            ndc_x(x1 + nx), ndc_y(y1 + ny),
            ndc_x(x0 - nx), ndc_y(y0 - ny),
            ndc_x(x1 - nx), ndc_y(y1 - ny),
        ]);
        self.draw(gl::GL_TRIANGLE_STRIP, 4, colour);
    }

    fn upload(&self, vertices: &[f32]) {
        unsafe {
            gl::glBufferData(
                gl::GL_ARRAY_BUFFER,
                (vertices.len() * std::mem::size_of::<f32>()) as isize,
                vertices.as_ptr() as *const std::ffi::c_void,
                gl::GL_STATIC_DRAW,
            );
        }
    }

    fn draw(&self, mode: u32, count: i32, colour: [f32; 4]) {
        unsafe {
            gl::glUniform4f(self.color, colour[0], colour[1], colour[2], colour[3]);
            gl::glDrawArrays(mode, 0, count);
        }
    }
}

impl Drop for UiRenderer {
    fn drop(&mut self) {
        unsafe { gl::glDeleteBuffers(1, &self.buffer) };
    }
}

// These helpers use the most recent viewport size stored in Engine; the UI is
// always drawn immediately after glViewport, so a single pair of globals is
// sufficient. They are set by `PreviewRenderer::draw_ui`.
use std::sync::atomic::{AtomicI32, Ordering};
static VIEW_WIDTH: AtomicI32 = AtomicI32::new(1);
static VIEW_HEIGHT: AtomicI32 = AtomicI32::new(1);

pub(crate) fn set_viewport_size(width: i32, height: i32) {
    VIEW_WIDTH.store(width.max(1), Ordering::Relaxed);
    VIEW_HEIGHT.store(height.max(1), Ordering::Relaxed);
}

fn ndc_x(x: f32) -> f32 {
    2.0 * x / VIEW_WIDTH.load(Ordering::Relaxed) as f32 - 1.0
}

fn ndc_y(y: f32) -> f32 {
    1.0 - 2.0 * y / VIEW_HEIGHT.load(Ordering::Relaxed) as f32
}
