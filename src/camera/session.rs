use super::characteristics::{Capabilities, Size};
use super::device::{CallbackContext, Device};
use super::error::{media, status, CameraError, Result};
use crate::config;
use crate::image::{CaptureFormat, CapturedImage};
use ndk_sys::{
    ACameraCaptureSession, ACameraCaptureSession_captureCallbacks,
    ACameraCaptureSession_stateCallbacks, ACameraOutputTarget, ACaptureSessionOutput,
    ACaptureSessionOutputContainer, AImageReader, AImageReader_ImageListener, ANativeWindow,
};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub struct CaptureQueue {
    pub sender: Sender<CapturedImage>,
}

struct CaptureData {
    queue: *const CaptureQueue,
    caps: Arc<Capabilities>,
    rotations: Mutex<VecDeque<i32>>,
}

struct ReaderContext {
    data: Arc<CaptureData>,
    format: CaptureFormat,
}

pub struct ImageReader {
    ptr: *mut AImageReader,
    window: *mut ANativeWindow,
    format: CaptureFormat,
    _listener: Box<AImageReader_ImageListener>,
    _context: Box<ReaderContext>,
}

impl ImageReader {
    pub fn new(size: Size, format: CaptureFormat, data: &Arc<CaptureData>) -> Result<Self> {
        let ndk_format = match format {
            CaptureFormat::Jpeg => ndk_sys::AIMAGE_FORMATS::AIMAGE_FORMAT_JPEG.0 as i32,
            CaptureFormat::Raw16 => ndk_sys::AIMAGE_FORMATS::AIMAGE_FORMAT_RAW16.0 as i32,
        };
        let max_images = match format {
            CaptureFormat::Jpeg => config::JPEG_MAX_IMAGES,
            CaptureFormat::Raw16 => config::RAW_MAX_IMAGES,
        };

        let mut ptr = MaybeUninit::<*mut AImageReader>::uninit();
        media(unsafe {
            ndk_sys::AImageReader_new(
                size.width,
                size.height,
                ndk_format,
                max_images,
                ptr.as_mut_ptr(),
            )
        })?;
        let ptr = unsafe { ptr.assume_init() };
        if ptr.is_null() {
            return Err(CameraError::NullPointer);
        }

        let mut window = MaybeUninit::<*mut ANativeWindow>::uninit();
        media(unsafe { ndk_sys::AImageReader_getWindow(ptr, window.as_mut_ptr()) })?;
        let window = unsafe { window.assume_init() };
        if window.is_null() {
            unsafe { ndk_sys::AImageReader_delete(ptr) };
            return Err(CameraError::NullPointer);
        }

        let context = Box::new(ReaderContext { data: data.clone(), format });
        let mut listener = Box::new(AImageReader_ImageListener {
            context: (&*context as *const ReaderContext) as *mut c_void,
            onImageAvailable: Some(on_image_available),
        });
        if let Err(error) = media(unsafe {
            ndk_sys::AImageReader_setImageListener(ptr, &mut *listener)
        }) {
            unsafe { ndk_sys::AImageReader_delete(ptr) };
            return Err(error);
        }

        Ok(Self { ptr, window, format, _listener: listener, _context: context })
    }

    pub fn window(&self) -> *mut ANativeWindow {
        self.window
    }

    pub fn format(&self) -> CaptureFormat {
        self.format
    }
}

impl Drop for ImageReader {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { ndk_sys::AImageReader_setImageListener(self.ptr, ptr::null_mut()) };
            unsafe { ndk_sys::AImageReader_delete(self.ptr) };
            self.ptr = ptr::null_mut();
        }
    }
}

unsafe extern "C" fn on_image_available(context: *mut c_void, reader: *mut AImageReader) {
    if context.is_null() || reader.is_null() {
        return;
    }
    let context = &*(context as *const ReaderContext);
    let queue = &*context.data.queue;

    for _ in 0..4 {
        let mut image = ptr::null_mut();
        if ndk_sys::AImageReader_acquireLatestImage(reader, &mut image)
            != ndk_sys::media_status_t::AMEDIA_OK
            || image.is_null()
        {
            break;
        }

        let mut bytes_ptr = ptr::null_mut();
        let mut length = 0;
        let mut width = 0;
        let mut height = 0;
        let mut row_stride = 0;
        let metadata_ok = media(unsafe {
            ndk_sys::AImage_getPlaneData(image, 0, &mut bytes_ptr, &mut length)
        })
        .is_ok()
            && media(unsafe { ndk_sys::AImage_getWidth(image, &mut width) }).is_ok()
            && media(unsafe { ndk_sys::AImage_getHeight(image, &mut height) }).is_ok()
            && media(unsafe {
                ndk_sys::AImage_getPlaneRowStride(image, 0, &mut row_stride)
            })
            .is_ok();

        if metadata_ok
            && !bytes_ptr.is_null()
            && length > 0
            && width > 0
            && height > 0
        {
            let device_rotation = {
                let mut rotations = context
                    .data
                    .rotations
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                rotations.pop_front().unwrap_or(0)
            };
            let bytes = std::slice::from_raw_parts(bytes_ptr, length as usize).to_vec();
            let _ = queue.sender.send(CapturedImage {
                format: context.format,
                bytes,
                width,
                height,
                row_stride,
                caps: context.data.caps.clone(),
                device_rotation,
            });
        }
        ndk_sys::AImage_delete(image);
    }
}

pub struct CaptureCallbacks {
    callbacks: Box<ACameraCaptureSession_captureCallbacks>,
}

impl CaptureCallbacks {
    pub fn new() -> Self {
        Self {
            callbacks: Box::new(ACameraCaptureSession_captureCallbacks {
                context: ptr::null_mut(),
                onCaptureStarted: None,
                onCaptureProgressed: None,
                onCaptureCompleted: None,
                onCaptureFailed: Some(on_capture_failed),
                onCaptureSequenceCompleted: None,
                onCaptureSequenceAborted: None,
                onCaptureBufferLost: None,
            }),
        }
    }

    pub fn set_context(&mut self, context: *mut c_void) {
        self.callbacks.context = context;
    }

    pub fn as_mut(&mut self) -> *mut ACameraCaptureSession_captureCallbacks {
        &mut *self.callbacks
    }
}

unsafe extern "C" fn on_capture_failed(
    context: *mut c_void,
    _session: *mut ACameraCaptureSession,
    _request: *mut ndk_sys::ACaptureRequest,
    _failure: *mut ndk_sys::ACameraCaptureFailure,
) {
    if !context.is_null() {
        let context = &*(context as *const CallbackContext);
        context.mark_error();
    }
}

pub struct Session {
    ptr: *mut ACameraCaptureSession,
    container: *mut ACaptureSessionOutputContainer,
    outputs: Vec<*mut ACaptureSessionOutput>,
    preview_target: *mut ACameraOutputTarget,
    // These callback structs point at `callback_context`, which is declared
    // after them so it is dropped last.
    callbacks: CaptureCallbacks,
    _state_callbacks: Box<ACameraCaptureSession_stateCallbacks>,
    callback_context: Box<CallbackContext>,
    pub jpeg_reader: ImageReader,
    pub raw_reader: Option<ImageReader>,
    capture_data: Arc<CaptureData>,
}

impl Session {
    pub fn new(
        device: &Device,
        preview_window: *mut ANativeWindow,
        preview_size: Size,
        jpeg_size: Size,
        raw_size: Option<Size>,
        queue: &Arc<CaptureQueue>,
        state: &Arc<super::device::DeviceState>,
        generation: u64,
        caps: Arc<Capabilities>,
    ) -> Result<Self> {
        if preview_window.is_null() {
            return Err(CameraError::NullPointer);
        }

        let capture_data = Arc::new(CaptureData {
            queue: Arc::as_ptr(queue),
            caps,
            rotations: Mutex::new(VecDeque::new()),
        });

        let mut container = MaybeUninit::<*mut ACaptureSessionOutputContainer>::uninit();
        status(unsafe {
            ndk_sys::ACaptureSessionOutputContainer_create(container.as_mut_ptr())
        })?;
        let container = unsafe { container.assume_init() };
        if container.is_null() {
            return Err(CameraError::NullPointer);
        }

        let jpeg_reader = match ImageReader::new(jpeg_size, CaptureFormat::Jpeg, &capture_data) {
            Ok(reader) => reader,
            Err(error) => {
                unsafe { ndk_sys::ACaptureSessionOutputContainer_free(container) };
                return Err(error);
            }
        };
        let raw_reader = match raw_size {
            Some(size) => match ImageReader::new(size, CaptureFormat::Raw16, &capture_data) {
                Ok(reader) => Some(reader),
                Err(error) => {
                    unsafe { ndk_sys::ACaptureSessionOutputContainer_free(container) };
                    return Err(error);
                }
            },
            None => None,
        };

        let mut windows = vec![(preview_window, preview_size), (jpeg_reader.window(), jpeg_size)];
        if let (Some(reader), Some(size)) = (&raw_reader, raw_size) {
            windows.push((reader.window(), size));
        }

        let mut outputs = Vec::with_capacity(windows.len());
        for (window, _size) in windows {
            let mut output = MaybeUninit::<*mut ACaptureSessionOutput>::uninit();
            let created = status(unsafe {
                ndk_sys::ACaptureSessionOutput_create(window, output.as_mut_ptr())
            });
            if let Err(error) = created {
                free_outputs(container, &outputs);
                return Err(error);
            }
            let output = unsafe { output.assume_init() };
            if output.is_null() {
                free_outputs(container, &outputs);
                return Err(CameraError::NullPointer);
            }
            if let Err(error) = status(unsafe {
                ndk_sys::ACaptureSessionOutputContainer_add(container, output)
            }) {
                unsafe { ndk_sys::ACaptureSessionOutput_free(output) };
                free_outputs(container, &outputs);
                return Err(error);
            }
            outputs.push(output);
        }

        let mut preview_target = MaybeUninit::<*mut ACameraOutputTarget>::uninit();
        if let Err(error) = status(unsafe {
            ndk_sys::ACameraOutputTarget_create(preview_window, preview_target.as_mut_ptr())
        }) {
            free_outputs(container, &outputs);
            return Err(error);
        }
        let preview_target = unsafe { preview_target.assume_init() };
        if preview_target.is_null() {
            free_outputs(container, &outputs);
            return Err(CameraError::NullPointer);
        }

        let callback_context = Box::new(CallbackContext::new(state, generation));
        let context_ptr = callback_context.as_ptr();
        let mut callbacks = CaptureCallbacks::new();
        callbacks.set_context(context_ptr);
        let state_callbacks = Box::new(ACameraCaptureSession_stateCallbacks {
            context: context_ptr,
            onClosed: Some(on_session_state),
            onReady: None,
            onActive: None,
        });

        let mut session = MaybeUninit::<*mut ACameraCaptureSession>::uninit();
        if let Err(error) = status(unsafe {
            ndk_sys::ACameraDevice_createCaptureSession(
                device.as_ptr(),
                container,
                &*state_callbacks,
                session.as_mut_ptr(),
            )
        }) {
            unsafe { ndk_sys::ACameraOutputTarget_free(preview_target) };
            free_outputs(container, &outputs);
            return Err(error);
        }
        let session_ptr = unsafe { session.assume_init() };
        if session_ptr.is_null() {
            unsafe { ndk_sys::ACameraOutputTarget_free(preview_target) };
            free_outputs(container, &outputs);
            return Err(CameraError::NullPointer);
        }

        Ok(Self {
            ptr: session_ptr,
            container,
            outputs,
            preview_target,
            callbacks,
            _state_callbacks: state_callbacks,
            callback_context,
            jpeg_reader,
            raw_reader,
            capture_data,
        })
    }

    pub fn ptr(&self) -> *mut ACameraCaptureSession {
        self.ptr
    }

    pub fn preview_target(&self) -> *mut ACameraOutputTarget {
        self.preview_target
    }

    pub fn reader_window(&self, format: CaptureFormat) -> Option<*mut ANativeWindow> {
        match format {
            CaptureFormat::Jpeg => Some(self.jpeg_reader.window()),
            CaptureFormat::Raw16 => self.raw_reader.as_ref().map(|reader| reader.window()),
        }
    }

    pub fn callback_context_ptr(&self) -> *mut c_void {
        self.callback_context.as_ptr()
    }

    pub fn callbacks_mut(&mut self) -> *mut ACameraCaptureSession_captureCallbacks {
        self.callbacks.as_mut()
    }

    pub fn push_capture_rotation(&self, rotation: i32) {
        let mut rotations = self
            .capture_data
            .rotations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        rotations.push_back(rotation);
    }

    pub fn pop_capture_rotation(&self) {
        let mut rotations = self
            .capture_data
            .rotations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        rotations.pop_back();
    }
}

fn free_outputs(container: *mut ACaptureSessionOutputContainer, outputs: &[*mut ACaptureSessionOutput]) {
    for output in outputs {
        if !output.is_null() {
            unsafe { ndk_sys::ACaptureSessionOutput_free(*output) };
        }
    }
    if !container.is_null() {
        unsafe { ndk_sys::ACaptureSessionOutputContainer_free(container) };
    }
}

unsafe extern "C" fn on_session_state(
    context: *mut c_void,
    _session: *mut ACameraCaptureSession,
) {
    if !context.is_null() {
        let context = &*(context as *const CallbackContext);
        context.mark_error();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Invalidate this generation before closing so the intentional onClosed
        // callback cannot be mistaken for an unexpected camera failure.
        self.callback_context.invalidate();
        if !self.ptr.is_null() {
            unsafe { ndk_sys::ACameraCaptureSession_close(self.ptr) };
            self.ptr = ptr::null_mut();
        }
        if !self.preview_target.is_null() {
            unsafe { ndk_sys::ACameraOutputTarget_free(self.preview_target) };
            self.preview_target = ptr::null_mut();
        }
        for output in self.outputs.drain(..) {
            if !output.is_null() {
                unsafe { ndk_sys::ACaptureSessionOutput_free(output) };
            }
        }
        if !self.container.is_null() {
            unsafe { ndk_sys::ACaptureSessionOutputContainer_free(self.container) };
            self.container = ptr::null_mut();
        }
    }
}
