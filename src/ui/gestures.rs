use super::controls::{self, Control};
use android_activity::input::{InputStatus, MotionAction, MotionEvent};
use std::time::Instant;

#[derive(Debug)]
pub enum GestureAction {
    Control(Control),
    Focus(f32, f32),
    Zoom(f32),
}

#[derive(Clone, Copy)]
struct TrackedPointer {
    id: i32,
    start_x: f32,
    start_y: f32,
    x: f32,
    y: f32,
    down_at: Instant,
}

pub struct GestureTracker {
    pointers: Vec<TrackedPointer>,
    pinch: Option<(f32, f32)>, // (initial distance, zoom at pinch start)
}

impl GestureTracker {
    pub fn new() -> Self {
        Self { pointers: Vec::new(), pinch: None }
    }

    pub fn handle(
        &mut self,
        event: &MotionEvent<'_>,
        width: i32,
        height: i32,
        current_zoom: f32,
    ) -> (InputStatus, Vec<GestureAction>) {
        let mut actions = Vec::new();
        let action = event.action();

        // `pointer_index()` is only meaningful for Up/Down variants. Reading
        // `pointer_at_index()` for Move/Cancel/Outside can panic on events that
        // have no changing pointer, so extract coordinates only when valid.
        let changed_pointer = match action {
            MotionAction::Down
            | MotionAction::PointerDown
            | MotionAction::Up
            | MotionAction::PointerUp => {
                let index = event.pointer_index();
                if index < event.pointer_count() {
                    let pointer = event.pointer_at_index(index);
                    Some((pointer.pointer_id(), pointer.x(), pointer.y()))
                } else {
                    None
                }
            }
            _ => None,
        };

        match action {
            MotionAction::Down => {
                self.pointers.clear();
                self.pinch = None;
                if let Some((id, x, y)) = changed_pointer {
                    self.push(id, x, y);
                }
            }
            MotionAction::PointerDown => {
                if let Some((id, x, y)) = changed_pointer {
                    self.push(id, x, y);
                }
                self.start_pinch(current_zoom);
            }
            MotionAction::Move => {
                for pointer in &mut self.pointers {
                    if let Some(current) = event.pointers().find(|p| p.pointer_id() == pointer.id) {
                        pointer.x = current.x();
                        pointer.y = current.y();
                    }
                }
                self.update_pinch(&mut actions);
            }
            MotionAction::Up | MotionAction::PointerUp => {
                self.update_positions(event);
                let id = match changed_pointer {
                    Some((id, _, _)) => id,
                    None => {
                        self.pointers.clear();
                        self.pinch = None;
                        return (InputStatus::Handled, actions);
                    }
                };
                if let Some(tap) = self.tap_action(id, width, height) {
                    actions.push(tap);
                }
                self.pointers.retain(|pointer| pointer.id != id);
                if self.pointers.len() < 2 {
                    self.pinch = None;
                }
            }
            MotionAction::Cancel | MotionAction::Outside => {
                self.pointers.clear();
                self.pinch = None;
            }
            _ => {}
        }

        (InputStatus::Handled, actions)
    }

    fn push(&mut self, id: i32, x: f32, y: f32) {
        self.pointers.push(TrackedPointer {
            id,
            start_x: x,
            start_y: y,
            x,
            y,
            down_at: Instant::now(),
        });
    }

    fn update_positions(&mut self, event: &MotionEvent<'_>) {
        for pointer in &mut self.pointers {
            if let Some(current) = event.pointers().find(|p| p.pointer_id() == pointer.id) {
                pointer.x = current.x();
                pointer.y = current.y();
            }
        }
    }

    fn start_pinch(&mut self, current_zoom: f32) {
        if self.pointers.len() < 2 {
            return;
        }
        let distance = distance(&self.pointers[0], &self.pointers[1]);
        if distance > 1.0 {
            self.pinch = Some((distance, current_zoom));
        }
    }

    fn update_pinch(&self, actions: &mut Vec<GestureAction>) {
        if let Some((initial_distance, initial_zoom)) = self.pinch {
            if self.pointers.len() >= 2 {
                let first = &self.pointers[0];
                let second = &self.pointers[1];
                let distance = distance(first, second);
                let zoom = initial_zoom * (distance / initial_distance);
                actions.push(GestureAction::Zoom(zoom));
            }
        }
    }

    fn tap_action(&self, id: i32, width: i32, height: i32) -> Option<GestureAction> {
        if self.pointers.len() != 1 {
            return None;
        }
        let pointer = self.pointers.first()?;
        if pointer.id != id {
            return None;
        }
        let moved = (pointer.x - pointer.start_x).hypot(pointer.y - pointer.start_y) > 16.0;
        if moved || pointer.down_at.elapsed().as_millis() > 400 {
            return None;
        }

        if let Some(control) = controls::hit_test(width, height, pointer.x, pointer.y) {
            return Some(GestureAction::Control(control));
        }
        if width <= 0 || height <= 0 {
            return None;
        }
        Some(GestureAction::Focus(
            (pointer.x / width as f32).clamp(0.0, 1.0),
            (pointer.y / height as f32).clamp(0.0, 1.0),
        ))
    }
}

fn distance(a: &TrackedPointer, b: &TrackedPointer) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}
