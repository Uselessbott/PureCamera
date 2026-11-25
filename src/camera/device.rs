use super::error::{status, CameraError, Result};
use super::manager::CameraManager;
use ndk_sys::{
    ACameraDevice, ACameraDevice_StateCallback, ACameraDevice_StateCallbacks,
    ACameraDevice_ErrorStateCallback, ACameraDevice_request_template, ACaptureRequest,
};
use std::ffi::{c_void, CStr};
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Flags written by the C callbacks and polled from the Rust main loop.
#[derive(Default)]
pub struct DeviceState {
    pub disconnected: AtomicBool,
    pub error: AtomicBool,
    // Callbacks carry the generation they belong to. Closing a device/session
    // invalidates the generation so a late callback cannot mark its successor
    // as broken.
    generation: AtomicU64,
}

impl DeviceState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_bad(&self) -> bool {
        self.disconnected.load(Ordering::Acquire) || self.error.load(Ordering::Acquire)
    }

    /// Clears only failure flags. The generation counter keeps increasing so
    /// stale native callbacks remain identifiable.
    pub fn clear(&self) {
        self.disconnected.store(false, Ordering::Release);
        self.error.store(false, Ordering::Release);
    }

    pub fn set_error(&self) {
        self.error.store(true, Ordering::Release);
    }

    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

/// Context passed to every native camera callback for one device/session pair.
pub struct CallbackContext {
    state: Arc<DeviceState>,
    generation: u64,
}

impl CallbackContext {
    pub fn new(state: &Arc<DeviceState>, generation: u64) -> Self {
        Self { state: state.clone(), generation }
    }

    pub fn as_ptr(&self) -> *mut c_void {
        (self as *const Self) as *mut c_void
    }

    pub fn invalidate(&self) {
        self.state.generation.fetch_add(1, Ordering::AcqRel);
    }

    fn is_current(&self) -> bool {
        self.state.generation() == self.generation
    }

    fn mark_disconnected(&self) {
        if self.is_current() {
            self.state.disconnected.store(true, Ordering::Release);
        }
    }

    fn mark_error(&self) {
        if self.is_current() {
            self.state.error.store(true, Ordering::Release);
        }
    }
}

pub struct Device {
    ptr: *mut ACameraDevice,
    // Callback structs must outlive the native device in case the
    // implementation keeps their pointer rather than copying them.
    _callbacks: Box<ACameraDevice_StateCallbacks>,
    // Declared after `_callbacks` so the context is dropped last.
    _context: Box<CallbackContext>,
}

impl Device {
    pub fn open(
        manager: &CameraManager,
        id: &CStr,
        state: &Arc<DeviceState>,
        generation: u64,
    ) -> Result<Self> {
        let context = Box::new(CallbackContext::new(state, generation));
        let mut callbacks = Box::new(ACameraDevice_StateCallbacks {
            context: context.as_ptr(),
            onDisconnected: Some(on_disconnected as ACameraDevice_StateCallback),
            onError: Some(on_error as ACameraDevice_ErrorStateCallback),
        });
        let mut device = MaybeUninit::<*mut ACameraDevice>::uninit();
        status(unsafe {
            ndk_sys::ACameraManager_openCamera(
                manager.as_ptr(),
                id.as_ptr(),
                &mut *callbacks,
                device.as_mut_ptr(),
            )
        })?;
        let ptr = unsafe { device.assume_init() };
        if ptr.is_null() {
            return Err(CameraError::NullPointer);
        }
        Ok(Self { ptr, _callbacks: callbacks, _context: context })
    }

    pub fn as_ptr(&self) -> *mut ACameraDevice {
        self.ptr
    }

    pub fn create_request(
        &self,
        template: ACameraDevice_request_template,
    ) -> Result<*mut ACaptureRequest> {
        let mut request = MaybeUninit::<*mut ACaptureRequest>::uninit();
        status(unsafe {
            ndk_sys::ACameraDevice_createCaptureRequest(
                self.ptr as *const ACameraDevice,
                template,
                request.as_mut_ptr(),
            )
        })?;
        let request = unsafe { request.assume_init() };
        if request.is_null() {
            Err(CameraError::NullPointer)
        } else {
            Ok(request)
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Invalidate callbacks before closing so teardown cannot be mistaken
        // for a real disconnect/error by the next device.
        self._context.invalidate();
        if !self.ptr.is_null() {
            unsafe { ndk_sys::ACameraDevice_close(self.ptr) };
            self.ptr = ptr::null_mut();
        }
    }
}

unsafe extern "C" fn on_disconnected(context: *mut c_void, _device: *mut ACameraDevice) {
    if !context.is_null() {
        let context = &*(context as *const CallbackContext);
        context.mark_disconnected();
    }
}

unsafe extern "C" fn on_error(
    context: *mut c_void,
    _device: *mut ACameraDevice,
    _error: std::ffi::c_int,
) {
    if !context.is_null() {
        let context = &*(context as *const CallbackContext);
        context.mark_error();
    }
}
