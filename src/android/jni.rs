//! JNI bridge written entirely in Rust.
//!
//! `android-activity` gives us the `JavaVM` and the `NativeActivity` instance.
//! We use those pointers directly with the `jni` crate; there is no Java or
//! Kotlin source file and no generated wrapper class in the APK.

use android_activity::AndroidApp;
use jni::objects::{GlobalRef, JObject, JValue};
use jni::sys::{jboolean, jobject};
use jni::{AttachGuard, JavaVM, JNIEnv};

/// Java objects needed to feed a `SurfaceTexture` into Camera2 and GLES.
pub struct SurfaceResources {
    pub texture: GlobalRef,
    pub surface: GlobalRef,
}

impl Drop for SurfaceResources {
    fn drop(&mut self) {
        // GlobalRefs release themselves. The corresponding ANativeWindow is
        // released by PreviewRenderer when the GLES context is torn down.
    }
}

/// Small helper around the JVM attached to the native activity thread.
pub struct JniBridge {
    vm: JavaVM,
    activity: jobject,
    // `AndroidApp` is cheaply cloneable. Holding a clone lets us dispatch
    // main-thread-only framework calls without a Java/Kotlin wrapper class.
    app: AndroidApp,
}

impl JniBridge {
    pub fn new(app: &AndroidApp) -> Result<Self, jni::errors::Error> {
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM)? };
        Ok(Self {
            vm,
            activity: app.activity_as_ptr() as jobject,
            app: app.clone(),
        })
    }

    pub(crate) fn attach(&self) -> Result<AttachGuard<'_>, jni::errors::Error> {
        self.vm.attach_current_thread()
    }

    pub(crate) fn activity<'local>(&self, _env: &JNIEnv<'local>) -> JObject<'local> {
        // `activity_as_ptr()` is a JNI global reference owned by NativeActivity.
        // It must not be deleted here.
        unsafe { JObject::from_raw(self.activity) }
    }

    pub fn toast(&self, message: &str) {
        let _ = self.toast_inner(message);
    }

    fn toast_inner(&self, message: &str) -> Result<(), jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(8, |env| {
            let activity = self.activity(env);
            let text = env.new_string(message)?;
            let toast = env.call_static_method(
                "android/widget/Toast",
                "makeText",
                "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;",
                &[
                    JValue::Object(&activity),
                    JValue::Object(&text),
                    JValue::Int(0), // Toast.LENGTH_SHORT
                ],
            )?;
            env.call_method(toast.l()?, "show", "()V", &[])?;
            Ok(())
        })
    }

    pub fn has_camera_permission(&self) -> Result<bool, jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(8, |env| {
            let activity = self.activity(env);
            let permission = env.new_string("android.permission.CAMERA")?;
            let granted = env.call_method(
                &activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&permission)],
            )?;
            // android.content.pm.PackageManager.PERMISSION_GRANTED == 0
            Ok(granted.i()? == 0)
        })
    }

    pub fn request_camera_permission(&self) -> Result<(), jni::errors::Error> {
        let app = self.app.clone();
        self.app.run_on_java_main_thread(Box::new(move || {
            let _ = request_camera_permission_on_main_thread(&app);
        }));
        Ok(())
    }

    /// Returns the display rotation in degrees: 0, 90, 180 or 270.
    pub fn display_rotation(&self) -> i32 {
        let rotation = self.display_rotation_inner().unwrap_or(0);
        rotation.rem_euclid(360)
    }

    fn display_rotation_inner(&self) -> Result<i32, jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(8, |env| {
            let activity = self.activity(env);
            let manager = env
                .call_method(
                    &activity,
                    "getWindowManager",
                    "()Landroid/view/WindowManager;",
                    &[],
                )?
                .l()?;
            let display = env
                .call_method(
                    &manager,
                    "getDefaultDisplay",
                    "()Landroid/view/Display;",
                    &[],
                )?
                .l()?;
            let rotation = env.call_method(&display, "getRotation", "()I", &[])?.i()?;
            Ok(rotation * 90)
        })
    }

    /// Creates `SurfaceTexture(false)` and a `Surface` backed by it.
    ///
    /// `false` means the texture is attached to the *current* GL context, so
    /// it must be created after `eglMakeCurrent` succeeds.
    pub fn create_surface_resources(&self) -> Result<SurfaceResources, jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(8, |env| {
            let texture_class = env.find_class("android/graphics/SurfaceTexture")?;
            let texture =
                env.new_object(texture_class, "(Z)V", &[JValue::Bool(0 as jboolean)])?;
            let texture_ref = env.new_global_ref(&texture)?;

            let surface_class = env.find_class("android/view/Surface")?;
            let surface = env.new_object(
                surface_class,
                "(Landroid/graphics/SurfaceTexture;)V",
                &[JValue::Object(&texture)],
            )?;
            let surface_ref = env.new_global_ref(&surface)?;

            Ok(SurfaceResources {
                texture: texture_ref,
                surface: surface_ref,
            })
        })
    }

    /// Sets the `SurfaceTexture` default buffer size to the selected preview size.
    pub fn set_surface_texture_size(
        &self,
        resources: &SurfaceResources,
        width: i32,
        height: i32,
    ) -> Result<(), jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(4, |env| {
            env.call_method(
                resources.texture.as_obj(),
                "setDefaultBufferSize",
                "(II)V",
                &[JValue::Int(width), JValue::Int(height)],
            )?;
            Ok(())
        })
    }

    /// Copies the latest camera frame into the GL external texture and returns
    /// the transform matrix that preserves the camera preview aspect ratio.
    pub fn update_surface_texture(
        &self,
        resources: &SurfaceResources,
        matrix: &mut [f32; 16],
    ) -> Result<bool, jni::errors::Error> {
        let mut guard = self.attach()?;
        guard.with_local_frame(8, |env| {
            let texture = resources.texture.as_obj();

            if env.call_method(texture, "updateTexImage", "()V", &[]).is_err() {
                if env.exception_check()? {
                    // A frame may not be available yet; this is not fatal.
                    env.exception_clear()?;
                }
                return Ok(false);
            }

            let array = env.new_float_array(16)?;
            env.call_method(
                texture,
                "getTransformMatrix",
                "([F)V",
                &[JValue::Object(&array)],
            )?;
            env.get_float_array_region(&array, 0, matrix)?;
            Ok(true)
        })
    }
}

/// `Activity.requestPermissions` is a main-thread-only framework call. The
/// native event loop runs on `android_main`'s dedicated thread, so dispatch the
/// JNI work through `AndroidApp::run_on_java_main_thread`.
fn request_camera_permission_on_main_thread(app: &AndroidApp) -> Result<(), jni::errors::Error> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM)? };
    let mut guard = vm.attach_current_thread()?;
    let activity_raw = app.activity_as_ptr() as jobject;
    if activity_raw.is_null() {
        return Ok(());
    }

    guard.with_local_frame(8, |env| {
        let activity = unsafe { JObject::from_raw(activity_raw) };
        let permission = env.new_string("android.permission.CAMERA")?;
        let permissions = env.new_object_array(1, "java/lang/String", JObject::null())?;
        env.set_object_array_element(&permissions, 0, &permission)?;
        env.call_method(
            &activity,
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[
                JValue::Object(&permissions),
                JValue::Int(crate::config::PERMISSION_REQUEST_CODE),
            ],
        )?;
        Ok(())
    })
}
