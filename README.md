# PureCamera

A minimal, privacy-focused Android camera whose application source is entirely
Rust. The APK has no Java or Kotlin code: `android.app.NativeActivity` loads the
Rust `cdylib` (`libpure_camera.so`) directly.

> Status: the audit fixes have been applied to the Rust source and Android build
> configuration. Nothing has been compiled, built, installed, tested, or
> deployed here. Device verification is still required.

## Design

- **Rust only**: `android-activity` + `jni` for framework calls, `ndk-sys` for
  Camera2/NDK and `AImageReader`, and hand-written Rust `extern "C"` bindings
  for EGL/GLES2 preview rendering.
- **Camera2**: device/session/request lifecycle is isolated in `src/camera/`.
- **Preview**: `SurfaceTexture` → `GL_TEXTURE_EXTERNAL_OES`; the
  `SurfaceTexture` transform matrix is applied in the shader, so the preview
  keeps the camera aspect ratio and fills the screen without stretching.
- **Capture**: JPEG bytes from `AImageReader` are copied byte-for-byte to
  MediaStore. There is no decode, resize, re-encode, sharpening, smoothing,
  beauty mode, colour filter, or computational/AI enhancement.
- **RAW**: when the device exposes `RAW_SENSOR`/RAW16, an optional DNG file is
  written from the original 16-bit raw bytes plus camera-reported metadata.
  A JPEG is never presented as raw sensor data. If RAW is unavailable, the app
  uses the highest-quality native JPEG the camera reports.
- **Storage**: Android `MediaStore` under `Pictures/PureCamera`. No storage
  permission is requested (minSdk 29+). `IS_PENDING` is cleared after writing,
  including on write/close failures.

## Privacy

- Only permission in the manifest: `android.permission.CAMERA`.
- No `INTERNET` permission, analytics, ads, telemetry, cloud services,
  accounts, tracking, background sync, or upload code.
- No gallery UI: only a short local `Toast` confirmation after saving.
- `android:allowBackup="false"` and `android:hasCode="false"`.

## Controls

- **Shutter** (bottom centre): capture.
- **Bottom-right button**: front/rear switch.
- **Bottom-left button**: flash Auto → On → Off.
- **Top-left minus/plus**: exposure compensation in camera-reported steps,
  applied to the camera immediately.
- **Top-right chip**: toggle RAW/DNG (disabled when unsupported).
- **Tap preview**: autofocus at that point.
- **Pinch**: digital zoom within the camera's reported range.

## Source layout

```text
src/lib.rs               cdylib crate root, event loop, lifecycle, capture dispatch
src/android/             Rust JNI bridge, permissions, MediaStore writes
src/camera/              manager, characteristics, device, session, requests
src/render/              EGL/GLES2 preview, minimal dark UI overlay
src/ui/                  gesture recognition and UI state
src/image/               JPEG passthrough and minimal DNG/TIFF writer
src/config.rs            metadata tags, formats, modes, limits
android/                 NativeActivity manifest/resources/Gradle project
```

## Building (not performed here)

You need a Rust Android target, cargo-ndk, an Android SDK/NDK, and Gradle.
The aliases in `.cargo/config.toml` package all three target ABIs into the
Gradle `jniLibs` directory. The Gradle `preBuild` guard checks that each ABI is
present before packaging:

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo android
cd android && gradle assembleDebug
```

For a single ABI during local development, override the guard:

```sh
cargo android-arm64
cd android && gradle assembleDebug -PpureCameraAbis=arm64-v8a
```

`assembleDebug` produces a debug-signed APK that can be installed directly with
`adb install`. The unsigned release APK is not installable; CI uploads it only
as an artifact for later signing. Release builds are never debug-signed. To
produce a signed release, provide all four signing properties:

```sh
gradle assembleRelease \
  -PRELEASE_STORE_FILE=/path/to/keystore.jks \
  -PRELEASE_STORE_PASSWORD=... \
  -PRELEASE_KEY_ALIAS=... \
  -PRELEASE_KEY_PASSWORD=...
```

The corresponding `RELEASE_*` environment variables are also accepted.

## Functionality requiring future compilation and device testing

These areas are written but must be validated on real devices/ABIs before any
claim of runtime correctness:

1. `cargo build`/`cargo ndk` and Gradle packaging for every target ABI, including
   the Gradle ABI guard and unsigned/signed release variants.
2. `NativeActivity` startup, main-thread permission dialog pause/resume, and
   lifecycle cleanup on disconnect, backgrounding, rotation, and window resize.
3. Proactive camera recovery after reported device/session errors, including
   stale-callback generation handling and retry backoff.
4. EGL/GLES2 shader compilation, shader cleanup on failure paths,
   `SurfaceTexture` transform/aspect ratio, and front-camera mirroring per device.
5. Camera2 stream configuration, flash modes, tap-to-focus regions, exposure
   steps, zoom crop regions, and neutral-processing tag support.
6. JPEG marker validation, EXIF preservation, byte-for-byte MediaStore writes,
   `is_pending` cleanup, and DNG validity in desktop/phone RAW converters
   (including CFA pattern mapping, 2x2 black-level metadata and row-stride
   stripping).
7. Gesture pointer-index safety for Move/Cancel/Outside events and immediate
   exposure application.

Because no build or device run was performed, metadata enum values, NDK FFI
signatures, JNI method descriptors, and device-specific RAW metadata should be
re-checked against the installed NDK during the first build.
