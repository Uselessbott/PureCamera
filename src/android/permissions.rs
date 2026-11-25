//! Camera permission handling.
//!
//! NativeActivity has no Java callback for `onRequestPermissionsResult`, so we
//! request the permission once and then re-check it on every `Resume` event.
//! The permission dialog pauses and resumes the activity, which gives us a
//! reliable, Java-source-free way to notice the user's answer. `JniBridge`
//! marshals the actual request onto the Java main/UI thread.

use super::JniBridge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Unknown,
    Granted,
    Requested,
    Denied,
}

pub struct PermissionController {
    state: PermissionState,
    notice_shown: bool,
}

impl PermissionController {
    pub fn new() -> Self {
        Self {
            state: PermissionState::Unknown,
            notice_shown: false,
        }
    }

    pub fn state(&self) -> PermissionState {
        self.state
    }

    /// Call from `MainEvent::Resume` and once at startup.
    pub fn refresh(&mut self, jni: &JniBridge) -> PermissionState {
        match jni.has_camera_permission() {
            Ok(true) => {
                self.state = PermissionState::Granted;
                self.notice_shown = false;
            }
            Ok(false) => {
                if self.state == PermissionState::Requested {
                    self.state = PermissionState::Denied;
                    if !self.notice_shown {
                        jni.toast("Camera permission is required");
                        self.notice_shown = true;
                    }
                } else if self.state != PermissionState::Denied {
                    self.state = PermissionState::Unknown;
                    if jni.request_camera_permission().is_ok() {
                        self.state = PermissionState::Requested;
                    }
                }
            }
            Err(_) => {
                // Keep the previous state; JNI failures are usually transient.
            }
        }
        self.state
    }

    pub fn reset(&mut self) {
        self.state = PermissionState::Unknown;
        self.notice_shown = false;
    }
}
