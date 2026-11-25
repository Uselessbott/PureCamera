use ndk_sys::{camera_status_t, media_status_t};

#[derive(Debug)]
pub enum CameraError {
    Status(i32),
    Media(i32),
    NullPointer,
    InvalidMetadata,
    NotSupported(&'static str),
    Jni(String),
    Io(String),
}

pub type Result<T> = std::result::Result<T, CameraError>;

impl From<jni::errors::Error> for CameraError {
    fn from(value: jni::errors::Error) -> Self {
        CameraError::Jni(value.to_string())
    }
}

impl std::fmt::Display for CameraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CameraError::Status(value) => write!(f, "Camera2 status error {value}"),
            CameraError::Media(value) => write!(f, "MediaNDK status error {value}"),
            CameraError::NullPointer => write!(f, "unexpected null native pointer"),
            CameraError::InvalidMetadata => write!(f, "invalid camera metadata"),
            CameraError::NotSupported(what) => write!(f, "{what} is not supported"),
            CameraError::Jni(message) => write!(f, "JNI error: {message}"),
            CameraError::Io(message) => write!(f, "I/O error: {message}"),
        }
    }
}

pub fn status(status: camera_status_t) -> Result<()> {
    if status.0 == ndk_sys::camera_status_t::ACAMERA_OK.0 {
        Ok(())
    } else {
        Err(CameraError::Status(status.0))
    }
}

pub fn media(status: media_status_t) -> Result<()> {
    if status.0 == ndk_sys::media_status_t::AMEDIA_OK.0 {
        Ok(())
    } else {
        Err(CameraError::Media(status.0))
    }
}
