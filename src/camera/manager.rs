use super::characteristics::Capabilities;
use super::error::{status, CameraError, Result};
use ndk_sys::{ACameraIdList, ACameraManager, ACameraMetadata};
use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::ptr;

pub struct CameraManager {
    ptr: *mut ACameraManager,
}

impl CameraManager {
    pub fn new() -> Result<Self> {
        let ptr = unsafe { ndk_sys::ACameraManager_create() };
        if ptr.is_null() {
            return Err(CameraError::NullPointer);
        }
        Ok(Self { ptr })
    }

    pub fn as_ptr(&self) -> *mut ACameraManager {
        self.ptr
    }

    pub fn camera_ids(&self) -> Result<Vec<CString>> {
        let mut list = MaybeUninit::<*mut ACameraIdList>::uninit();
        status(unsafe { ndk_sys::ACameraManager_getCameraIdList(self.ptr, list.as_mut_ptr()) })?;
        let list = unsafe { list.assume_init() };
        if list.is_null() {
            return Err(CameraError::NullPointer);
        }

        let ids = unsafe {
            let list_ref = &*list;
            let mut result = Vec::with_capacity(list_ref.numCameras as usize);
            for index in 0..list_ref.numCameras as usize {
                let id_ptr = *list_ref.cameraIds.add(index);
                if !id_ptr.is_null() {
                    result.push(CStr::from_ptr(id_ptr).to_owned());
                }
            }
            result
        };
        unsafe { ndk_sys::ACameraManager_deleteCameraIdList(list) };
        Ok(ids)
    }

    pub fn capabilities(&self, id: &CStr) -> Result<Capabilities> {
        let mut metadata = MaybeUninit::<*mut ACameraMetadata>::uninit();
        status(unsafe {
            ndk_sys::ACameraManager_getCameraCharacteristics(
                self.ptr,
                id.as_ptr(),
                metadata.as_mut_ptr(),
            )
        })?;
        let metadata = unsafe { metadata.assume_init() };
        if metadata.is_null() {
            return Err(CameraError::NullPointer);
        }
        let result = unsafe { Capabilities::from_metadata(metadata, id.as_ptr()) };
        unsafe { ndk_sys::ACameraMetadata_free(metadata) };
        result
    }

    /// Selects the first front camera, falling back to the first back camera.
    pub fn select_camera(&self) -> Result<(CString, Capabilities)> {
        let ids = self.camera_ids()?;
        if ids.is_empty() {
            return Err(CameraError::NotSupported("no camera"));
        }
        let mut best: Option<(CString, Capabilities)> = None;
        for id in ids {
            let caps = self.capabilities(&id)?;
            let is_front = caps.lens_facing == super::characteristics::LensFacing::Front;
            if is_front {
                return Ok((id, caps));
            }
            if best.is_none() {
                best = Some((id, caps));
            }
        }
        best.ok_or(CameraError::NotSupported("no usable camera"))
    }

    pub fn select_facing(
        &self,
        want_front: bool,
    ) -> Result<(CString, Capabilities)> {
        let ids = self.camera_ids()?;
        let wanted = if want_front {
            super::characteristics::LensFacing::Front
        } else {
            super::characteristics::LensFacing::Back
        };
        let mut fallback = None;
        for id in ids {
            let caps = self.capabilities(&id)?;
            if caps.lens_facing == wanted {
                return Ok((id, caps));
            }
            if fallback.is_none() {
                fallback = Some((id, caps));
            }
        }
        fallback.ok_or(CameraError::NotSupported("requested camera direction"))
    }
}

impl Drop for CameraManager {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { ndk_sys::ACameraManager_delete(self.ptr) };
            self.ptr = ptr::null_mut();
        }
    }
}
