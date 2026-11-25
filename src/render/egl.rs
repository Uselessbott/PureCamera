use std::ffi::c_void;
use std::ptr;

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContextHandle = *mut c_void;
type EglSurface = *mut c_void;

const EGL_DEFAULT_DISPLAY: EglDisplay = ptr::null_mut();
const EGL_NO_CONTEXT: EglContextHandle = ptr::null_mut();
const EGL_NO_SURFACE: EglSurface = ptr::null_mut();
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_NONE: i32 = 0x3038;

#[link(name = "EGL")]
extern "C" {
    fn eglGetDisplay(display: EglDisplay) -> EglDisplay;
    fn eglInitialize(display: EglDisplay, major: *mut i32, minor: *mut i32) -> u32;
    fn eglChooseConfig(
        display: EglDisplay,
        attributes: *const i32,
        configs: *mut EglConfig,
        config_size: i32,
        num_configs: *mut i32,
    ) -> u32;
    fn eglCreateContext(
        display: EglDisplay,
        config: EglConfig,
        share: EglContextHandle,
        attributes: *const i32,
    ) -> EglContextHandle;
    fn eglCreateWindowSurface(
        display: EglDisplay,
        config: EglConfig,
        window: EglDisplay,
        attributes: *const i32,
    ) -> EglSurface;
    fn eglMakeCurrent(
        display: EglDisplay,
        draw: EglSurface,
        read: EglSurface,
        context: EglContextHandle,
    ) -> u32;
    fn eglSwapBuffers(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroySurface(display: EglDisplay, surface: EglSurface) -> u32;
    fn eglDestroyContext(display: EglDisplay, context: EglContextHandle) -> u32;
    fn eglTerminate(display: EglDisplay) -> u32;
}

pub struct EglContext {
    display: EglDisplay,
    config: EglConfig,
    context: EglContextHandle,
    surface: EglSurface,
}

impl EglContext {
    pub fn new(native_window: *mut c_void) -> Result<Self, String> {
        unsafe {
            let display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
            if display.is_null() {
                return Err("eglGetDisplay failed".into());
            }
            let mut major = 0;
            let mut minor = 0;
            if eglInitialize(display, &mut major, &mut minor) == 0 {
                return Err("eglInitialize failed".into());
            }

            let config_attributes = [
                EGL_SURFACE_TYPE,
                EGL_WINDOW_BIT,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_ES2_BIT,
                EGL_NONE,
            ];
            let mut config: EglConfig = ptr::null_mut();
            let mut config_count = 0;
            if eglChooseConfig(
                display,
                config_attributes.as_ptr(),
                &mut config,
                1,
                &mut config_count,
            ) == 0 || config_count == 0
            {
                eglTerminate(display);
                return Err("eglChooseConfig failed".into());
            }

            let context_attributes = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
            let context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes.as_ptr());
            if context == EGL_NO_CONTEXT {
                eglTerminate(display);
                return Err("eglCreateContext failed".into());
            }

            let surface = eglCreateWindowSurface(display, config, native_window, ptr::null());
            if surface == EGL_NO_SURFACE {
                eglDestroyContext(display, context);
                eglTerminate(display);
                return Err("eglCreateWindowSurface failed".into());
            }

            if eglMakeCurrent(display, surface, surface, context) == 0 {
                eglDestroySurface(display, surface);
                eglDestroyContext(display, context);
                eglTerminate(display);
                return Err("eglMakeCurrent failed".into());
            }

            Ok(Self { display, config, context, surface })
        }
    }

    pub fn make_current(&self) {
        unsafe {
            eglMakeCurrent(self.display, self.surface, self.surface, self.context);
        }
    }

    pub fn swap(&self) {
        unsafe {
            eglSwapBuffers(self.display, self.surface);
        }
    }

    pub fn config(&self) -> EglConfig {
        self.config
    }
}

impl Drop for EglContext {
    fn drop(&mut self) {
        unsafe {
            eglMakeCurrent(self.display, ptr::null_mut(), ptr::null_mut(), EGL_NO_CONTEXT);
            eglDestroySurface(self.display, self.surface);
            eglDestroyContext(self.display, self.context);
            eglTerminate(self.display);
        }
    }
}
