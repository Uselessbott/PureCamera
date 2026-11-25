//! Pure-Rust Android camera entry point.
//!
//! `NativeActivity` loads this cdylib and calls `android_main`. All framework
//! interaction is performed through Rust JNI calls; the APK contains no Java,
//! Kotlin, generated source or bytecode.

mod android;
mod camera;
mod config;
mod image;
mod render;
mod ui;

use android_activity::input::{InputEvent, InputStatus};
use android_activity::{AndroidApp, MainEvent, PollEvent};
use android::permissions::PermissionState;
use android::{JniBridge, PermissionController};
use camera::{CameraController, CaptureQueue};
use image::{build_dng, save_jpeg, CaptureFormat, CapturedImage};
use render::{gl, PreviewRenderer};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ui::{GestureAction, GestureTracker, UiState};

pub struct Engine {
    // Drop order matters: camera callbacks must die before the capture queue,
    // and GL/SurfaceTexture resources must die before the JavaVM wrapper.
    camera: Option<CameraController>,
    renderer: Option<PreviewRenderer>,
    ui: UiState,
    gestures: GestureTracker,
    caps: Option<camera::Capabilities>,
    preview_matrix: [f32; 16],
    width: i32,
    height: i32,
    running: bool,
    queue: Arc<CaptureQueue>,
    captures: Receiver<CapturedImage>,
    permissions: PermissionController,
    jni: JniBridge,
}

impl Engine {
    fn new(app: &AndroidApp) -> Option<Self> {
        let jni = JniBridge::new(app).ok()?;
        let mut permissions = PermissionController::new();
        permissions.refresh(&jni);

        let (sender, captures) = mpsc::channel();
        let queue = Arc::new(CaptureQueue { sender });

        Some(Self {
            camera: None,
            renderer: None,
            ui: UiState::new(),
            gestures: GestureTracker::new(),
            caps: None,
            preview_matrix: gl::identity(),
            width: 1,
            height: 1,
            running: true,
            queue,
            captures,
            permissions,
            jni,
        })
    }

    fn on_event(&mut self, app: &AndroidApp, event: PollEvent<'_>) {
        match event {
            PollEvent::Main(MainEvent::InputAvailable) => self.handle_input(app),
            PollEvent::Main(MainEvent::InitWindow { .. }) => self.init_window(app),
            PollEvent::Main(MainEvent::TerminateWindow { .. }) => {
                self.stop_camera();
                self.renderer = None;
            }
            PollEvent::Main(MainEvent::WindowResized { .. })
            | PollEvent::Main(MainEvent::RedrawNeeded { .. }) => self.resize_window(app),
            PollEvent::Main(MainEvent::Start)
            | PollEvent::Main(MainEvent::Resume { .. }) => {
                self.permissions.refresh(&self.jni);
                self.ensure_camera(app);
            }
            PollEvent::Main(MainEvent::Pause) | PollEvent::Main(MainEvent::Stop) => {
                self.stop_camera();
            }
            PollEvent::Main(MainEvent::LowMemory) => {
                self.stop_camera();
                self.notify("Low memory: camera paused");
            }
            PollEvent::Main(MainEvent::Destroy) => self.shutdown(),
            _ => {}
        }
    }

    fn init_window(&mut self, app: &AndroidApp) {
        self.create_renderer(app);
        self.ensure_camera(app);
    }

    fn resize_window(&mut self, app: &AndroidApp) {
        let window = match app.native_window() {
            Some(window) => window,
            None => return,
        };
        let width = if window.width() > 0 {
            window.width()
        } else {
            app.content_rect().right - app.content_rect().left
        };
        let height = if window.height() > 0 {
            window.height()
        } else {
            app.content_rect().bottom - app.content_rect().top
        };
        if width <= 0 || height <= 0 {
            return;
        }

        if self.renderer.is_some() && width == self.width && height == self.height {
            self.ensure_camera(app);
            return;
        }
        self.create_renderer(app);
        self.ensure_camera(app);
    }

    fn create_renderer(&mut self, app: &AndroidApp) {
        let window = match app.native_window() {
            Some(window) => window,
            None => return,
        };
        let width = if window.width() > 0 { window.width() } else { self.width };
        let height = if window.height() > 0 { window.height() } else { self.height };
        if width <= 0 || height <= 0 {
            return;
        }

        self.stop_camera();
        self.renderer = None;
        match PreviewRenderer::new(&self.jni, &window) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                self.width = width;
                self.height = height;
                self.preview_matrix = gl::identity();
            }
            Err(error) => self.notify(format!("Preview: {error}")),
        }
    }

    fn ensure_camera(&mut self, app: &AndroidApp) {
        if self.permissions.state() != PermissionState::Granted {
            if self.permissions.state() == PermissionState::Denied {
                self.notify("Camera permission denied");
            }
            return;
        }
        if self.renderer.is_none() {
            self.create_renderer(app);
            if self.renderer.is_none() {
                return;
            }
        }

        if self.camera.is_none() {
            match CameraController::new(self.queue.clone(), self.ui.front_camera) {
                Ok(camera) => {
                    let raw_supported = camera.raw_supported();
                    let front_camera = camera.front_camera();
                    self.caps = Some(camera.capabilities().clone());
                    self.camera = Some(camera);
                    self.ui.front_camera = front_camera;
                    self.ui.raw_supported = raw_supported;
                }
                Err(error) => {
                    self.notify(format!("Camera: {error}"));
                    return;
                }
            }
        }

        let preview_size = self
            .caps
            .as_ref()
            .and_then(|caps| caps.preview_for_display(self.width, self.height));
        if let (Some(renderer), Some(size)) = (&self.renderer, preview_size) {
            renderer.set_preview_size(&self.jni, size.width, size.height);
        }

        let window = match &self.renderer {
            Some(renderer) => renderer.native_window(),
            None => return,
        };
        let camera = match self.camera.as_mut() {
            Some(camera) => camera,
            None => return,
        };

        let start_result = if camera.is_bad() || !camera.is_started() {
            camera.start(window, self.width, self.height)
        } else {
            Ok(())
        };
        if let Err(error) = start_result {
            self.notify(format!("Camera start: {error}"));
            return;
        }

        let settings = camera.apply_ui(self.ui.flash, self.ui.exposure_steps, self.ui.zoom);
        if let Err(error) = settings {
            self.notify(format!("Camera settings: {error}"));
        }
    }

    fn stop_camera(&mut self) {
        if let Some(camera) = &mut self.camera {
            camera.stop();
        }
    }

    fn shutdown(&mut self) {
        self.stop_camera();
        self.renderer = None;
        self.running = false;
    }

    /// Locally visible confirmation/error. There is no gallery or remote call.
    fn notify(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.ui.set_message(message.clone(), now_ms());
        self.jni.toast(&message);
    }

    fn handle_input(&mut self, app: &AndroidApp) {
        let mut events = match app.input_events_iter() {
            Ok(events) => events,
            Err(_) => return,
        };
        let width = self.width;
        let height = self.height;
        let zoom = self.ui.zoom;
        let mut actions = Vec::new();

        while events.next(|event| match event {
            InputEvent::MotionEvent(motion) => {
                let (status, found) = self.gestures.handle(motion, width, height, zoom);
                actions.extend(found);
                status
            }
            _ => InputStatus::Unhandled,
        }) {}

        for action in actions {
            self.apply_gesture(app, action);
        }
    }

    fn apply_gesture(&mut self, app: &AndroidApp, action: GestureAction) {
        match action {
            GestureAction::Control(ui::Control::Capture) => {
                self.ui.capture_requested = true;
            }
            GestureAction::Control(ui::Control::SwitchCamera) => self.switch_camera(app),
            GestureAction::Control(ui::Control::Flash) => {
                self.ui.cycle_flash();
                self.ensure_camera(app);
            }
            GestureAction::Control(ui::Control::Raw) => {
                if self.ui.raw_supported {
                    self.ui.raw_mode = !self.ui.raw_mode;
                    let message = if self.ui.raw_mode { "RAW/DNG enabled" } else { "JPEG mode" };
                    self.notify(message);
                } else {
                    self.notify("RAW not supported on this camera");
                }
            }
            GestureAction::Control(ui::Control::ExposureDown) => {
                self.change_exposure(-1);
                // Apply immediately through the same path used at startup.
                self.ensure_camera(app);
            }
            GestureAction::Control(ui::Control::ExposureUp) => {
                self.change_exposure(1);
                self.ensure_camera(app);
            }
            GestureAction::Focus(x, y) => {
                self.ui.focus_request = Some((x, y));
                let focus_result = match &mut self.camera {
                    Some(camera) => camera.focus_at(x, y),
                    None => Ok(()),
                };
                if let Err(error) = focus_result {
                    self.notify(format!("Focus: {error}"));
                }
            }
            GestureAction::Zoom(zoom) => {
                let max = self
                    .caps
                    .as_ref()
                    .map(|caps| caps.max_digital_zoom.max(1.0).min(config::MAX_ZOOM))
                    .unwrap_or(1.0);
                self.ui.zoom = zoom.clamp(1.0, max);
                self.ensure_camera(app);
            }
        }
    }

    fn switch_camera(&mut self, _app: &AndroidApp) {
        let window = match &self.renderer {
            Some(renderer) => renderer.native_window(),
            None => return,
        };
        let result = match &mut self.camera {
            Some(camera) => camera.switch_camera(window, self.width, self.height),
            None => return,
        };
        match result {
            Ok(()) => {
                let (front, raw, caps) = match &self.camera {
                    Some(camera) => (
                        camera.front_camera(),
                        camera.raw_supported(),
                        camera.capabilities().clone(),
                    ),
                    None => return,
                };
                self.ui.front_camera = front;
                self.ui.raw_supported = raw;
                self.ui.raw_mode &= raw;
                self.ui.zoom = 1.0;
                self.ui.exposure_steps = 0;
                self.caps = Some(caps);
                self.notify("Camera switched");
            }
            Err(error) => self.notify(format!("Switch failed: {error}")),
        }
    }

    fn change_exposure(&mut self, direction: i32) {
        let range = self
            .caps
            .as_ref()
            .map(|caps| caps.exposure_range)
            .unwrap_or((-6, 6));
        self.ui.exposure_steps = (self.ui.exposure_steps + direction).clamp(range.0, range.1);
    }

    fn tick(&mut self) {
        let recovery_error = match &mut self.camera {
            Some(camera) => camera.tick().err().map(|error| error.to_string()),
            None => None,
        };
        if let Some(error) = recovery_error {
            // Keep failures locally visible without a Toast on every retry.
            self.ui.set_message(format!("Camera recovery: {error}"), now_ms());
        }
        self.expire_message();
        self.issue_capture();
        self.save_captures();
    }

    fn expire_message(&mut self) {
        if self.ui.message.is_some() && now_ms().saturating_sub(self.ui.message_at_ms) > 2500 {
            self.ui.message = None;
            self.ui.message_at_ms = 0;
        }
    }

    fn issue_capture(&mut self) {
        if !self.ui.capture_requested {
            return;
        }
        self.ui.capture_requested = false;
        let camera = match &mut self.camera {
            Some(camera) if camera.is_started() => camera,
            _ => {
                self.notify("Camera not ready");
                return;
            }
        };
        let format = if self.ui.raw_mode && self.ui.raw_supported {
            CaptureFormat::Raw16
        } else {
            CaptureFormat::Jpeg
        };
        let rotation = self.jni.display_rotation();
        let result = camera.capture(format, rotation);
        if let Err(error) = result {
            self.notify(format!("Capture failed: {error}"));
        }
    }

    fn save_captures(&mut self) {
        while let Ok(image) = self.captures.try_recv() {
            let saved = match image.format {
                CaptureFormat::Jpeg => save_jpeg(&self.jni, &image),
                CaptureFormat::Raw16 => {
                    // Use the capabilities and device rotation captured with
                    // this image, not the UI's current camera state.
                    let orientation =
                        camera::image_orientation(&image.caps, image.device_rotation);
                    match build_dng(&image, orientation) {
                        Ok(dng) => android::mediastore::save_to_pictures(
                            &self.jni,
                            &dng,
                            "image/x-adobe-dng",
                            "dng",
                        ),
                        Err(error) => Err(error),
                    }
                }
            };
            match saved {
                Ok(name) => self.notify(format!("Saved {name}")),
                Err(error) => self.notify(format!("Save failed: {error}")),
            }
        }
    }

    fn render(&mut self) {
        let (renderer, camera) = match (&self.renderer, &mut self.camera) {
            (Some(renderer), Some(camera)) => (renderer, camera),
            _ => return,
        };
        if !camera.is_started() {
            return;
        }
        let matrix = if renderer.update_texture(&self.jni, &mut self.preview_matrix) {
            &self.preview_matrix
        } else {
            &gl::IDENTITY
        };
        renderer.render(self.width, self.height, matrix, &self.ui);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut engine = match Engine::new(&app) {
        Some(engine) => engine,
        None => return,
    };

    while engine.running {
        app.poll_events(Some(Duration::from_millis(16)), |event| {
            engine.on_event(&app, event);
        });
        engine.tick();
        engine.render();
    }
}
