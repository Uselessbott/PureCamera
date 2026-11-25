//! Tiny hand-written GLES2 binding. No additional graphics crate is needed.

use std::ffi::{c_char, c_void};
use std::ptr;

pub const GL_TEXTURE_2D: u32 = 0x0de1;
pub const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8d65;
pub const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
pub const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
pub const GL_TEXTURE_WRAP_S: u32 = 0x2802;
pub const GL_TEXTURE_WRAP_T: u32 = 0x2803;
pub const GL_CLAMP_TO_EDGE: u32 = 0x812f;
pub const GL_LINEAR: u32 = 0x2601;
pub const GL_ARRAY_BUFFER: u32 = 0x8892;
pub const GL_STATIC_DRAW: u32 = 0x88e4;
pub const GL_FLOAT: u32 = 0x1406;
pub const GL_TRIANGLES: u32 = 4;
pub const GL_TRIANGLE_STRIP: u32 = 5;
pub const GL_TRIANGLE_FAN: u32 = 6;
pub const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
pub const GL_VERTEX_SHADER: u32 = 0x8b31;
pub const GL_FRAGMENT_SHADER: u32 = 0x8b30;
pub const GL_COMPILE_STATUS: u32 = 0x8b81;
pub const GL_LINK_STATUS: u32 = 0x8b82;
pub const GL_TEXTURE0: u32 = 0x84c0;
pub const GL_BLEND: u32 = 0x0be2;
pub const GL_SRC_ALPHA: u32 = 0x0302;
pub const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;

#[link(name = "GLESv2")]
extern "C" {
    pub fn glViewport(x: i32, y: i32, width: i32, height: i32);
    pub fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
    pub fn glClear(mask: u32);
    pub fn glEnable(cap: u32);
    pub fn glDisable(cap: u32);
    pub fn glBlendFunc(source: u32, destination: u32);

    pub fn glGenTextures(count: i32, textures: *mut u32);
    pub fn glBindTexture(target: u32, texture: u32);
    pub fn glTexParameteri(target: u32, parameter: u32, value: i32);
    pub fn glActiveTexture(texture: u32);
    pub fn glDeleteTextures(count: i32, textures: *const u32);

    pub fn glGenBuffers(count: i32, buffers: *mut u32);
    pub fn glBindBuffer(target: u32, buffer: u32);
    pub fn glBufferData(target: u32, size: isize, data: *const c_void, usage: u32);
    pub fn glDeleteBuffers(count: i32, buffers: *const u32);
    pub fn glEnableVertexAttribArray(index: u32);
    pub fn glVertexAttribPointer(
        index: u32,
        size: i32,
        kind: u32,
        normalized: u8,
        stride: i32,
        pointer: *const c_void,
    );

    pub fn glCreateShader(kind: u32) -> u32;
    pub fn glShaderSource(shader: u32, count: i32, source: *const *const c_char, length: *const i32);
    pub fn glCompileShader(shader: u32);
    pub fn glGetShaderiv(shader: u32, parameter: u32, value: *mut i32);
    pub fn glDeleteShader(shader: u32);

    pub fn glCreateProgram() -> u32;
    pub fn glAttachShader(program: u32, shader: u32);
    pub fn glLinkProgram(program: u32);
    pub fn glGetProgramiv(program: u32, parameter: u32, value: *mut i32);
    pub fn glUseProgram(program: u32);
    pub fn glDeleteProgram(program: u32);
    pub fn glGetUniformLocation(program: u32, name: *const c_char) -> i32;
    pub fn glGetAttribLocation(program: u32, name: *const c_char) -> i32;
    pub fn glUniformMatrix4fv(location: i32, count: i32, transpose: u8, value: *const f32);
    pub fn glUniform1i(location: i32, value: i32);
    pub fn glUniform4f(location: i32, x: f32, y: f32, z: f32, w: f32);
    pub fn glDrawArrays(mode: u32, first: i32, count: i32);
}

pub struct Program {
    pub program: u32,
    vertex: u32,
    fragment: u32,
}

impl Program {
    pub fn new(vertex_source: &str, fragment_source: &str) -> Result<Self, String> {
        unsafe {
            let vertex = compile(GL_VERTEX_SHADER, vertex_source)?;
            let fragment = match compile(GL_FRAGMENT_SHADER, fragment_source) {
                Ok(fragment) => fragment,
                Err(error) => {
                    glDeleteShader(vertex);
                    return Err(error);
                }
            };
            let program = glCreateProgram();
            if program == 0 {
                glDeleteShader(vertex);
                glDeleteShader(fragment);
                return Err("glCreateProgram failed".into());
            }
            glAttachShader(program, vertex);
            glAttachShader(program, fragment);
            glLinkProgram(program);
            let mut linked = 0;
            glGetProgramiv(program, GL_LINK_STATUS, &mut linked);
            if linked == 0 {
                glDeleteProgram(program);
                glDeleteShader(vertex);
                glDeleteShader(fragment);
                return Err("shader link failed".into());
            }
            Ok(Self { program, vertex, fragment })
        }
    }

    pub fn uniform(&self, name: &str) -> i32 {
        let name = std::ffi::CString::new(name).unwrap();
        unsafe { glGetUniformLocation(self.program, name.as_ptr()) }
    }

    pub fn attribute(&self, name: &str) -> i32 {
        let name = std::ffi::CString::new(name).unwrap();
        unsafe { glGetAttribLocation(self.program, name.as_ptr()) }
    }

    pub fn use_program(&self) {
        unsafe { glUseProgram(self.program) };
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        unsafe {
            glDeleteProgram(self.program);
            glDeleteShader(self.vertex);
            glDeleteShader(self.fragment);
        }
    }
}

unsafe fn compile(kind: u32, source: &str) -> Result<u32, String> {
    let shader = glCreateShader(kind);
    if shader == 0 {
        return Err("glCreateShader failed".into());
    }
    let source = match std::ffi::CString::new(source) {
        Ok(source) => source,
        Err(_) => {
            glDeleteShader(shader);
            return Err("shader contains NUL".into());
        }
    };
    let sources = [source.as_ptr()];
    let lengths = [source.as_bytes().len() as i32];
    glShaderSource(shader, 1, sources.as_ptr(), lengths.as_ptr());
    glCompileShader(shader);
    let mut compiled = 0;
    glGetShaderiv(shader, GL_COMPILE_STATUS, &mut compiled);
    if compiled == 0 {
        glDeleteShader(shader);
        return Err("shader compile failed".into());
    }
    Ok(shader)
}

pub static IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

pub fn identity() -> [f32; 16] {
    IDENTITY
}

// Keep `ptr` referenced: some NDK headers type EGLNativeWindowType as void*.
const _: *const c_void = ptr::null();
