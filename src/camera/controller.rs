//! High-level camera lifecycle: device, session, requests and callbacks.
//!
//! This module owns all `ACameraDevice`/`ACameraCaptureSession` state. The UI
//! layer only sends small commands (flash, zoom, focus, capture) and never
//! touches NDK camera pointers directly.

use super::characteristics::{Capabilities, LensFacing, Size};
use super::device::{Device, DeviceState};
use super::error::{status, CameraError, Result};
use super::manager::CameraManager;
use super::requests::{self, FlashMode, FocusPoint};
use super::session::{CaptureCallbacks, CaptureQueue, Session};
use crate::config;
use crate::image::CaptureFormat;
use ndk_sys::{ACaptureRequest, ACameraDevice_request_template, ANativeWindow};
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct CameraController {
    // Drop order: device/session first, then state/callbacks, queue last.
    // Device::drop invalidates native callbacks before Session::drop runs.
    device: Option<Device>,
    session: Option<Session>,
    device_state: Arc<DeviceState>,
    capture_callbacks: CaptureCallbacks,
    camera_id: CString,
    caps: Arc<Capabilities>,
    want_front: bool,
    flash: FlashMode,
    exposure_steps: i32,
    zoom: f32,
    focus: FocusPoint,
    af_clear_at: Option<Instant>,
    preview_request: Option<*mut ACaptureRequest>,
    preview_window: *mut ANativeWindow,
    preview_width: i32,
    preview_height: i32,
    recovery_at: Option<Instant>,
    queue: Arc<CaptureQueue>,
    manager: CameraManager,
}

impl CameraController {
    pub fn new(queue: Arc<CaptureQueue>, want_front: bool) -> Result<Self> {
        let manager = CameraManager::new()?;
        let (camera_id, caps) = manager.select_facing(want_front)?;
        let caps = Arc::new(caps);
        let want_front = caps.lens_facing == LensFacing::Front;
        let device_state = Arc::new(DeviceState::new());
        Ok(Self {
            device: None,
            session: None,
            device_state,
            capture_callbacks: CaptureCallbacks::new(),
            camera_id,
            caps,
            want_front,
            flash: FlashMode::Auto,
            exposure_steps: 0,
            zoom: 1.0,
            focus: FocusPoint { region: [0; 5], active: false },
            af_clear_at: None,
            preview_request: None,
            preview_window: ptr::null_mut(),
            preview_width: 0,
            preview_height: 0,
            recovery_at: None,
            queue,
            manager,
        })
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    pub fn capabilities_arc(&self) -> Arc<Capabilities> {
        self.caps.clone()
    }

    pub fn raw_supported(&self) -> bool {
        self.caps.raw_capable && self.caps.largest_raw().is_some()
    }

    pub fn front_camera(&self) -> bool {
        self.want_front
    }

    pub fn is_bad(&self) -> bool {
        self.device_state.is_bad()
    }

    pub fn is_started(&self) -> bool {
        self.device.is_some() && self.session.is_some()
    }

    /// Preview size selected for the current display.
    pub fn preview_size(&self, width: i32, height: i32) -> Option<Size> {
        self.caps.preview_for_display(width, height)
    }

    pub fn start(
        &mut self,
        preview_window: *mut ANativeWindow,
        width: i32,
        height: i32,
    ) -> Result<()> {
        self.stop();
        self.device_state.clear();
        let generation = self.device_state.next_generation();
        self.preview_window = preview_window;
        self.preview_width = width;
        self.preview_height = height;

        let result = self.start_after_stop(preview_window, width, height, generation);
        if let Err(error) = result {
            self.stop();
            self.device_state.set_error();
            Err(error)
        } else {
            Ok(())
        }
    }

    fn start_after_stop(
        &mut self,
        preview_window: *mut ANativeWindow,
        width: i32,
        height: i32,
        generation: u64,
    ) -> Result<()> {
        let device = Device::open(&self.manager, &self.camera_id, &self.device_state, generation)?;
        let preview_size = self
            .caps
            .preview_for_display(width, height)
            .ok_or(CameraError::NotSupported("preview size"))?;
        let jpeg_size = self
            .caps
            .largest_jpeg()
            .ok_or(CameraError::NotSupported("JPEG size"))?;
        let raw_size = if self.raw_supported() { self.caps.largest_raw() } else { None };

        let session = Session::new(
            &device,
            preview_window,
            preview_size,
            jpeg_size,
            raw_size,
            &self.queue,
            &self.device_state,
            generation,
            self.caps.clone(),
        )?;
        self.capture_callbacks.set_context(session.callback_context_ptr());
        self.device = Some(device);
        self.session = Some(session);
        self.update_repeating()
    }

    pub fn stop(&mut self) {
        self.capture_callbacks.set_context(ptr::null_mut());
        if let Some(session) = &self.session {
            // Best-effort graceful stop before the session is closed.
            let _ = status(unsafe {
                ndk_sys::ACameraCaptureSession_stopRepeating(session.ptr())
            });
        }
        if let Some(request) = self.preview_request.take() {
            if !request.is_null() {
                unsafe { ndk_sys::ACaptureRequest_free(request) };
            }
        }
        self.session = None;
        self.device = None;
        // Deliberately do not clear `device_state` here: a genuine
        // disconnect/error must survive lifecycle teardown and be recovered
        // by the next `start()`.
    }

    pub fn switch_camera(
        &mut self,
        preview_window: *mut ANativeWindow,
        width: i32,
        height: i32,
    ) -> Result<()> {
        let requested_front = !self.want_front;
        let (id, caps) = self.manager.select_facing(requested_front)?;
        let actual_front = caps.lens_facing == LensFacing::Front;
        self.stop();
        self.camera_id = id;
        self.caps = Arc::new(caps);
        self.want_front = actual_front;
        self.zoom = 1.0;
        self.exposure_steps = 0;
        self.focus.active = false;
        self.af_clear_at = None;
        self.start(preview_window, width, height)
    }

    pub fn apply_ui(&mut self, flash: FlashMode, exposure_steps: i32, zoom: f32) -> Result<()> {
        let max_zoom = self.caps.max_digital_zoom.max(1.0).min(config::MAX_ZOOM);
        let clamped_zoom = zoom.clamp(1.0, max_zoom);
        let changed = self.flash != flash
            || self.exposure_steps != exposure_steps
            || (self.zoom - clamped_zoom).abs() > f32::EPSILON;

        if changed && self.session.is_some() {
            // Only commit UI values that the camera actually accepted.
            self.update_repeating_with(flash, exposure_steps, clamped_zoom, self.focus)?;
        }
        self.flash = flash;
        self.exposure_steps = exposure_steps;
        self.zoom = clamped_zoom;
        Ok(())
    }

    pub fn focus_at(&mut self, normalized_x: f32, normalized_y: f32) -> Result<()> {
        let crop = self.caps.active_array.cropped(self.zoom);
        let focus = FocusPoint {
            region: requests::focus_region(normalized_x, normalized_y, crop),
            active: true,
        };
        if self.session.is_some() {
            self.update_repeating_with(self.flash, self.exposure_steps, self.zoom, focus)?;
        }
        self.focus = focus;
        self.af_clear_at = Some(Instant::now() + Duration::from_millis(config::AF_TRIGGER_CLEAR_MS));
        Ok(())
    }

    /// Clears a one-shot autofocus trigger and proactively recovers from a
    /// device/session error or disconnect reported by the native callbacks.
    pub fn tick(&mut self) -> Result<()> {
        if self.is_bad() {
            return self.recover();
        }

        if let Some(deadline) = self.af_clear_at {
            if Instant::now() >= deadline {
                let cleared = FocusPoint { active: false, ..self.focus };
                let result = if self.session.is_some() {
                    self.update_repeating_with(
                        self.flash,
                        self.exposure_steps,
                        self.zoom,
                        cleared,
                    )
                } else {
                    Ok(())
                };
                if result.is_ok() {
                    self.focus = cleared;
                }
                self.af_clear_at = None;
                result?;
            }
        }
        Ok(())
    }

    fn recover(&mut self) -> Result<()> {
        let now = Instant::now();
        if let Some(last) = self.recovery_at {
            if now.duration_since(last) < Duration::from_millis(config::CAMERA_RECOVERY_RETRY_MS) {
                return Ok(());
            }
        }
        self.recovery_at = Some(now);

        if self.preview_window.is_null() || self.preview_width <= 0 || self.preview_height <= 0 {
            return Err(CameraError::NotSupported("preview window unavailable"));
        }
        let result = self.start(self.preview_window, self.preview_width, self.preview_height);
        if result.is_ok() {
            self.recovery_at = None;
        }
        result
    }

    pub fn capture(&mut self, format: CaptureFormat, device_rotation: i32) -> Result<()> {
        // Copy raw pointers and shared references first so no long-lived Rust
        // borrow of `self` overlaps the mutable borrow of the callbacks.
        let session = self
            .session
            .as_ref()
            .ok_or(CameraError::NotSupported("session not ready"))?;
        let device_ptr = self
            .device
            .as_ref()
            .ok_or(CameraError::NotSupported("camera not open"))?
            .as_ptr();
        let session_ptr = session.ptr();
        let target_window = session
            .reader_window(format)
            .ok_or(CameraError::NotSupported("capture format"))?;
        let callbacks_ptr = self.capture_callbacks.as_mut();

        let device = unsafe { &*device_ptr };
        let caps = &self.caps;
        let flash = self.flash;
        let exposure = self.exposure_steps;
        let zoom = self.zoom;
        let focus = self.focus;
        let rotation = device_rotation.rem_euclid(360);

        let mut request = device.create_request(ACameraDevice_request_template::TEMPLATE_STILL_CAPTURE)?;
        let mut target = MaybeUninit::<*mut ndk_sys::ACameraOutputTarget>::uninit();
        if let Err(error) = status(unsafe {
            ndk_sys::ACameraOutputTarget_create(target_window, target.as_mut_ptr())
        }) {
            unsafe { ndk_sys::ACaptureRequest_free(request) };
            return Err(error);
        }
        let target = unsafe { target.assume_init() };
        if target.is_null() {
            unsafe { ndk_sys::ACaptureRequest_free(request) };
            return Err(CameraError::NullPointer);
        }

        session.push_capture_rotation(rotation);
        let mut result = (|| -> Result<()> {
            status(unsafe { ndk_sys::ACaptureRequest_addTarget(request, target) })?;
            requests::apply_neutral(request, caps, flash, exposure, zoom, focus)?;
            requests::apply_still(request, caps, flash, rotation)?;
            Ok(())
        })();

        if result.is_ok() {
            let mut requests = [request];
            let mut sequence = 0;
            result = status(unsafe {
                ndk_sys::ACameraCaptureSession_capture(
                    session_ptr,
                    callbacks_ptr,
                    1,
                    requests.as_mut_ptr(),
                    &mut sequence,
                )
            });
        }
        if result.is_err() {
            session.pop_capture_rotation();
        }

        unsafe {
            ndk_sys::ACameraOutputTarget_free(target);
            ndk_sys::ACaptureRequest_free(request);
        }
        result
    }

    fn update_repeating(&mut self) -> Result<()> {
        self.update_repeating_with(self.flash, self.exposure_steps, self.zoom, self.focus)
    }

    fn update_repeating_with(
        &mut self,
        flash: FlashMode,
        exposure_steps: i32,
        zoom: f32,
        focus: FocusPoint,
    ) -> Result<()> {
        let (device_ptr, session_ptr, preview_target) = match (&self.device, &self.session) {
            (Some(device), Some(session)) => {
                (device.as_ptr(), session.ptr(), session.preview_target())
            }
            _ => return Ok(()),
        };

        let previous = self.preview_request.take();
        let device = unsafe { &*device_ptr };
        let caps = &self.caps;
        let request = match device.create_request(ACameraDevice_request_template::TEMPLATE_PREVIEW) {
            Ok(request) => request,
            Err(error) => {
                self.preview_request = previous;
                return Err(error);
            }
        };

        let result = (|| -> Result<()> {
            status(unsafe { ndk_sys::ACaptureRequest_addTarget(request, preview_target) })?;
            requests::apply_neutral(request, caps, flash, exposure_steps, zoom, focus)?;
            let mut requests = [request];
            let mut sequence = 0;
            status(unsafe {
                ndk_sys::ACameraCaptureSession_setRepeatingRequest(
                    session_ptr,
                    std::ptr::null_mut(),
                    1,
                    requests.as_mut_ptr(),
                    &mut sequence,
                )
            })
        })();

        if let Err(error) = result {
            unsafe { ndk_sys::ACaptureRequest_free(request) };
            self.preview_request = previous;
            return Err(error);
        }
        if let Some(previous) = previous {
            if !previous.is_null() {
                unsafe { ndk_sys::ACaptureRequest_free(previous) };
            }
        }
        self.preview_request = Some(request);
        Ok(())
    }
}

impl Drop for CameraController {
    fn drop(&mut self) {
        self.stop();
    }
}
