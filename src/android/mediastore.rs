//! MediaStore writes, performed from Rust through JNI.
//!
//! Images go to `Pictures/PureCamera`. `IS_PENDING` is set while the bytes are
//! written and cleared afterwards, which is the documented MediaStore pattern
//! on API 29+ and requires no storage permission.

use super::JniBridge;
use jni::objects::{JObject, JValue};
use jni::JNIEnv;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
enum SaveError {
    Jni(jni::errors::Error),
    Message(String),
}

impl From<jni::errors::Error> for SaveError {
    fn from(value: jni::errors::Error) -> Self {
        SaveError::Jni(value)
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Jni(error) => write!(f, "{error}"),
            SaveError::Message(message) => write!(f, "{message}"),
        }
    }
}

/// Saves original camera bytes to MediaStore and returns the display name.
pub fn save_to_pictures(
    jni: &JniBridge,
    bytes: &[u8],
    mime_type: &str,
    extension: &str,
) -> Result<String, String> {
    let mut guard = jni.attach().map_err(|e| e.to_string())?;
    let env = &mut *guard;

    let result: Result<String, SaveError> = env.with_local_frame(32, |env| {
        let activity = jni.activity(env);
        let resolver = env
            .call_method(
                &activity,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;

        let external_uri = env
            .get_static_field(
                "android/provider/MediaStore$Images$Media",
                "EXTERNAL_CONTENT_URI",
                "Landroid/net/Uri;",
            )?
            .l()?;
        if external_uri.is_null() {
            return Err(SaveError::Message("MediaStore URI unavailable".into()));
        }

        let name = format!("PureCamera_{}.{}", timestamp_compact(), extension);
        let values = env.new_object("android/content/ContentValues", "()V", &[])?;

        put_string(env, &values, "_display_name", &name)?;
        put_string(env, &values, "mime_type", mime_type)?;
        put_string(env, &values, "relative_path", crate::config::MEDIA_SUBDIRECTORY)?;
        put_int(env, &values, "is_pending", 1)?;

        let uri = env
            .call_method(
                &resolver,
                "insert",
                "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;",
                &[JValue::Object(&external_uri), JValue::Object(&values)],
            )?
            .l()?;
        if uri.is_null() {
            return Err(SaveError::Message("MediaStore insert returned null".into()));
        }

        // From here on a row exists with is_pending=1. Every path must try to
        // clear it, including write/close failures and pending-row update errors.
        let write_result = (|| -> Result<(), SaveError> {
            let stream = env
                .call_method(
                    &resolver,
                    "openOutputStream",
                    "(Landroid/net/Uri;)Ljava/io/OutputStream;",
                    &[JValue::Object(&uri)],
                )?
                .l()?;
            if stream.is_null() {
                return Err(SaveError::Message(
                    "MediaStore openOutputStream returned null".into(),
                ));
            }

            let wrote = write_bytes(env, &stream, bytes);
            // Always try to close the stream, even if writing failed.
            let closed = env.call_method(&stream, "close", "()V", &[]).map(|_| ());
            wrote?;
            closed?;
            Ok(())
        })();

        // A pending JNI exception would make the cleanup update fail too.
        clear_exception(env);
        let cleanup_result = clear_pending(env, &resolver, &uri);

        match (write_result, cleanup_result) {
            (Ok(()), Ok(())) => Ok(name),
            (Err(write_error), Ok(())) => Err(write_error),
            (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
            (Err(write_error), Err(cleanup_error)) => Err(SaveError::Message(format!(
                "{write_error}; clearing is_pending failed: {cleanup_error}"
            ))),
        }
    });

    match result {
        Ok(name) => Ok(name),
        Err(error) => {
            clear_exception(env);
            Err(error.to_string())
        }
    }
}

fn clear_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

fn clear_pending(
    env: &mut JNIEnv<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
) -> Result<(), SaveError> {
    let values = env.new_object("android/content/ContentValues", "()V", &[])?;
    put_int(env, &values, "is_pending", 0)?;

    let null_string: JObject<'_> = JObject::null();
    let updated = env
        .call_method(
            resolver,
            "update",
            "(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I",
            &[
                JValue::Object(uri),
                JValue::Object(&values),
                JValue::Object(&null_string),
                JValue::Object(&null_string),
            ],
        )?
        .i()?;
    if updated <= 0 {
        return Err(SaveError::Message(
            "MediaStore did not clear is_pending".into(),
        ));
    }
    Ok(())
}

fn write_bytes(
    env: &mut JNIEnv<'_>,
    stream: &JObject<'_>,
    bytes: &[u8],
) -> jni::errors::Result<()> {
    // Chunked writes keep peak memory low for full-resolution JPEG/DNG files.
    const CHUNK: usize = 256 * 1024;
    for chunk in bytes.chunks(CHUNK) {
        let array = env.byte_array_from_slice(chunk)?;
        let write = env
            .call_method(stream, "write", "([B)V", &[JValue::Object(array.as_ref())])
            .map(|_| ());
        // Delete each chunk array immediately; a long MediaStore write must not
        // accumulate one JNI local reference per chunk.
        let deleted = env.delete_local_ref(array);
        write?;
        deleted?;
    }
    env.call_method(stream, "flush", "()V", &[]).map(|_| ())
}

fn put_string(
    env: &mut JNIEnv<'_>,
    values: &JObject<'_>,
    key: &str,
    value: &str,
) -> jni::errors::Result<()> {
    let key = env.new_string(key)?;
    let value = env.new_string(value)?;
    env.call_method(
        values,
        "put",
        "(Ljava/lang/String;Ljava/lang/String;)V",
        &[JValue::Object(&key), JValue::Object(&value)],
    )
    .map(|_| ())
}

fn put_int(
    env: &mut JNIEnv<'_>,
    values: &JObject<'_>,
    key: &str,
    value: i32,
) -> jni::errors::Result<()> {
    let key = env.new_string(key)?;
    let boxed = env
        .call_static_method(
            "java/lang/Integer",
            "valueOf",
            "(I)Ljava/lang/Integer;",
            &[JValue::Int(value)],
        )?
        .l()?;
    env.call_method(
        values,
        "put",
        "(Ljava/lang/String;Ljava/lang/Integer;)V",
        &[JValue::Object(&key), JValue::Object(&boxed)],
    )
    .map(|_| ())
}

/// UTC timestamp without external date/time dependencies.
fn timestamp_compact() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let remainder = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = remainder / 3600;
    let minute = (remainder % 3600) / 60;
    let second = remainder % 60;
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        year, month, day, hour, minute, second
    )
}

/// Howard Hinnant's civil calendar conversion, days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}
