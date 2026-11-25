use super::egl::EglContext;
use super::gl::{self, Program};
use super::ui::UiRenderer;
use crate::android::{JniBridge, SurfaceResources};
use crate::ui::UiState;
use ndk::native_window::NativeWindow;
use std::ffi::c_void;

const VERTEX: &str = "
attribute vec2 aPosition;
attribute vec2 aTexCoord;
varying vec2 vTexCoord;
uniform mat4 uTransform;
void main() {
    vTexCoord = (uTransform * vec4(aTexCoord, 0.0, 1.0)).xy;
    gl_Position = vec4(aPosition, 0.0, 1.0);
}
";

const FRAGMENT: &str = "
#extension GL_OES_EGL_image_external : require
precision mediump float;
varying vec2 vTexCoord;
uniform samplerExternalOES uCameraTexture;
void main() {
    gl_FragColor = texture2D(uCameraTexture, vTexCoord);
}
";

pub struct PreviewRenderer {
    program: Program,
    texture: u32,
    quad: u32,
    position_location: i32,
    texcoord_location: i32,
    transform_location: i32,
    texture_location: i32,
    ui: UiRenderer,
    surface_resources: SurfaceResources,
    native_window: *mut ndk_sys::ANativeWindow,
    // Declared last so the GL/EGL context is destroyed after all GL objects.
    egl: EglContext,
}

impl PreviewRenderer {
    pub fn new(jni: &JniBridge, window: &NativeWindow) -> Result<Self, String> {
        let egl = EglContext::new(window.ptr().as_ptr() as *mut c_void)?;
        let surface_resources = jni
            .create_surface_resources()
            .map_err(|error| error.to_string())?;

        let native_window = unsafe {
            let guard = jni.attach().map_err(|error| error.to_string())?;
            let env = guard.get_raw() as *mut jni::sys::JNIEnv;
            ndk_sys::ANativeWindow_fromSurface(
                env as *mut _,
                surface_resources.surface.as_obj().as_raw(),
            )
        };
        if native_window.is_null() {
            return Err("ANativeWindow_fromSurface failed".into());
        }

        // The camera preview size is negotiated in Rust; the corresponding
        // SurfaceTexture buffer size is applied by `set_preview_size`.
        let built = (|| -> Result<Self, String> {
            let program = Program::new(VERTEX, FRAGMENT)?;
            let position_location = program.attribute("aPosition");
            let texcoord_location = program.attribute("aTexCoord");
            let transform_location = program.uniform("uTransform");
            let texture_location = program.uniform("uCameraTexture");

            let mut texture = 0;
            let mut quad = 0;
            unsafe {
                gl::glGenTextures(1, &mut texture);
                gl::glBindTexture(gl::GL_TEXTURE_EXTERNAL_OES, texture);
                gl::glTexParameteri(gl::GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR as i32);
                gl::glTexParameteri(gl::GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR as i32);
                gl::glTexParameteri(gl::GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_WRAP_S, gl::GL_CLAMP_TO_EDGE as i32);
                gl::glTexParameteri(gl::GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_WRAP_T, gl::GL_CLAMP_TO_EDGE as i32);

                // x, y, u, v. The SurfaceTexture transform is applied in the shader.
                let vertices: [f32; 16] = [
                    -1.0, -1.0, 0.0, 0.0,
                     1.0, -1.0, 1.0, 0.0,
                    -1.0,  1.0, 0.0, 1.0,
                     1.0,  1.0, 1.0, 1.0,
                ];
                gl::glGenBuffers(1, &mut quad);
                gl::glBindBuffer(gl::GL_ARRAY_BUFFER, quad);
                gl::glBufferData(
                    gl::GL_ARRAY_BUFFER,
                    (vertices.len() * std::mem::size_of::<f32>()) as isize,
                    vertices.as_ptr() as *const c_void,
                    gl::GL_STATIC_DRAW,
                );
            }

            let ui = match UiRenderer::new() {
                Ok(ui) => ui,
                Err(error) => {
                    unsafe {
                        gl::glDeleteBuffers(1, &quad);
                        gl::glDeleteTextures(1, &texture);
                    }
                    return Err(error);
                }
            };
            Ok(Self {
                program,
                texture,
                quad,
                position_location,
                texcoord_location,
                transform_location,
                texture_location,
                ui,
                surface_resources,
                native_window,
                egl,
            })
        })();

        if built.is_err() {
            unsafe { ndk_sys::ANativeWindow_release(native_window) };
        }
        built
    }

    /// Raw ANativeWindow used as the Camera2 preview output.
    pub fn native_window(&self) -> *mut ndk_sys::ANativeWindow {
        self.native_window
    }

    /// Matches the `SurfaceTexture` producer buffer size to the Camera2 stream.
    pub fn set_preview_size(&self, jni: &JniBridge, width: i32, height: i32) -> bool {
        jni.set_surface_texture_size(&self.surface_resources, width, height)
            .is_ok()
    }

    pub fn update_texture(&self, jni: &JniBridge, matrix: &mut [f32; 16]) -> bool {
        // `updateTexImage` requires the SurfaceTexture's GL context to be current.
        self.egl.make_current();
        jni.update_surface_texture(&self.surface_resources, matrix)
            .unwrap_or(false)
    }

    pub fn render(&self, width: i32, height: i32, matrix: &[f32; 16], ui_state: &UiState) {
        self.egl.make_current();
        unsafe {
            gl::glViewport(0, 0, width, height);
            gl::glClearColor(0.0, 0.0, 0.0, 1.0);
            gl::glClear(gl::GL_COLOR_BUFFER_BIT);

            self.program.use_program();
            gl::glActiveTexture(gl::GL_TEXTURE0);
            gl::glBindTexture(gl::GL_TEXTURE_EXTERNAL_OES, self.texture);
            if self.texture_location >= 0 {
                gl::glUniform1i(self.texture_location, 0);
            }
            if self.transform_location >= 0 {
                gl::glUniformMatrix4fv(self.transform_location, 1, 0, matrix.as_ptr());
            }

            gl::glBindBuffer(gl::GL_ARRAY_BUFFER, self.quad);
            if self.position_location >= 0 {
                gl::glEnableVertexAttribArray(self.position_location as u32);
                gl::glVertexAttribPointer(
                    self.position_location as u32,
                    2,
                    gl::GL_FLOAT,
                    0,
                    16,
                    std::ptr::null(),
                );
            }
            if self.texcoord_location >= 0 {
                gl::glEnableVertexAttribArray(self.texcoord_location as u32);
                gl::glVertexAttribPointer(
                    self.texcoord_location as u32,
                    2,
                    gl::GL_FLOAT,
                    0,
                    16,
                    8 as *const c_void,
                );
            }
            gl::glDrawArrays(gl::GL_TRIANGLE_STRIP, 0, 4);
        }

        super::ui::set_viewport_size(width, height);
        self.ui.draw(width, height, ui_state);
        self.egl.swap();
    }
}

impl Drop for PreviewRenderer {
    fn drop(&mut self) {
        self.egl.make_current();
        unsafe {
            gl::glDeleteBuffers(1, &self.quad);
            gl::glDeleteTextures(1, &self.texture);
        }
        if !self.native_window.is_null() {
            unsafe { ndk_sys::ANativeWindow_release(self.native_window) };
            self.native_window = std::ptr::null_mut();
        }
    }
}
