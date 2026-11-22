//! Android: the game data packed inside the app, and a place for saves.
//!
//! `pack android` puts the data folder in the APK's assets as
//! `IronHelixData/`, with `index.txt` listing its files (an APK's assets
//! can't be listed by folder). The game reads them through Android's asset
//! manager. (miniquad's own asset loader never frees what it reads, which a
//! game reading hundreds of movies can't afford.)

use std::ffi::{CStr, CString};
use std::io;
use std::path::PathBuf;

use macroquad::miniquad::native::android::{ACTIVITY, attach_jni_env, ndk_sys};
use helixdb::datapack::FOLDER;

unsafe extern "C" {
    fn AAssetManager_fromJava(env: *mut ndk_sys::JNIEnv, asset_manager: ndk_sys::jobject) -> *mut ndk_sys::AAssetManager;
}

/// The app's asset manager (kept alive by a global reference).
struct Assets(*mut ndk_sys::AAssetManager);

// The asset manager may be used from any thread.
unsafe impl Send for Assets {}
unsafe impl Sync for Assets {}

impl Assets {
    fn open() -> Option<Self> {
        unsafe {
            let env = attach_jni_env();
            let get_class = (**env).GetObjectClass.unwrap();
            let get_method = (**env).GetMethodID.unwrap();
            let call_object = (**env).CallObjectMethod.unwrap();
            let global_ref = (**env).NewGlobalRef.unwrap();
            let mid = get_method(env, get_class(env, ACTIVITY), c"getAssets".as_ptr(), c"()Landroid/content/res/AssetManager;".as_ptr());
            let manager = call_object(env, ACTIVITY, mid);
            if manager.is_null() {
                return None;
            }
            let mgr = AAssetManager_fromJava(env, global_ref(env, manager));
            (!mgr.is_null()).then_some(Assets(mgr))
        }
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let name = CString::new(path).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad asset name"))?;
        unsafe {
            let asset = ndk_sys::AAssetManager_open(self.0, name.as_ptr(), ndk_sys::AASSET_MODE_STREAMING as _);
            if asset.is_null() {
                return Err(io::Error::new(io::ErrorKind::NotFound, format!("no asset {path}")));
            }
            let len = ndk_sys::AAsset_getLength64(asset).max(0) as usize;
            let mut data = vec![0u8; len];
            let mut done = 0;
            while done < len {
                let n = ndk_sys::AAsset_read(asset, data[done..].as_mut_ptr() as _, (len - done) as _);
                if n <= 0 {
                    break;
                }
                done += n as usize;
            }
            ndk_sys::AAsset_close(asset);
            if done < len {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!("short read of {path}")));
            }
            Ok(data)
        }
    }
}

/// Panic messages to Android's log (logcat, tag "SAPP"): Android drops
/// what a program writes to stderr.
pub fn log_panics() {
    std::panic::set_hook(Box::new(|info| {
        let msg = CString::new(format!("Iron Helix panicked: {info}").replace('\0', " ")).unwrap_or_default();
        unsafe { macroquad::miniquad::native::android::console_error(msg.as_ptr()) };
    }));
}

/// Read the game data from the app's assets from now on. Without packed
/// data (an APK built with no data folder) nothing changes.
pub fn use_packed_data() {
    let Some(assets) = Assets::open() else { return };
    let Ok(index) = assets.read(&format!("{FOLDER}/index.txt")) else { return };
    let index = String::from_utf8_lossy(&index).into_owned();
    let source = helixdb::vfs::Indexed::new(FOLDER, &index, move |path: &str| assets.read(path));
    helixdb::vfs::set_source(Box::new(source));
}

/// Close the app (miniquad doesn't on Android when the game's loop ends,
/// which would leave a black screen): `Activity.finishAndRemoveTask()`.
pub fn close() {
    unsafe {
        let env = attach_jni_env();
        let get_class = (**env).GetObjectClass.unwrap();
        let get_method = (**env).GetMethodID.unwrap();
        let call_void = (**env).CallVoidMethod.unwrap();
        let mid = get_method(env, get_class(env, ACTIVITY), c"finishAndRemoveTask".as_ptr(), c"()V".as_ptr());
        call_void(env, ACTIVITY, mid);
    }
}

/// Keep the screen from dimming and sleeping, or let it again. Calls the
/// `setKeepScreenOn` that `pack android` adds to miniquad's activity.
pub fn keep_screen_on(on: bool) {
    unsafe {
        let env = attach_jni_env();
        let get_class = (**env).GetObjectClass.unwrap();
        let get_method = (**env).GetMethodID.unwrap();
        let call_void = (**env).CallVoidMethod.unwrap();
        let mid = get_method(env, get_class(env, ACTIVITY), c"setKeepScreenOn".as_ptr(), c"(Z)V".as_ptr());
        if mid.is_null() {
            // An activity without it: carry on (and clear Java's exception).
            (**env).ExceptionClear.unwrap()(env);
            return;
        }
        call_void(env, ACTIVITY, mid, on as std::ffi::c_int);
    }
}

/// The app's private folder (`Context.getFilesDir()`), for saved games.
pub fn files_dir() -> Option<PathBuf> {
    unsafe {
        let env = attach_jni_env();
        let get_class = (**env).GetObjectClass.unwrap();
        let get_method = (**env).GetMethodID.unwrap();
        let call_object = (**env).CallObjectMethod.unwrap();
        let utf_chars = (**env).GetStringUTFChars.unwrap();
        let release_chars = (**env).ReleaseStringUTFChars.unwrap();
        let mid = get_method(env, get_class(env, ACTIVITY), c"getFilesDir".as_ptr(), c"()Ljava/io/File;".as_ptr());
        let dir = call_object(env, ACTIVITY, mid);
        if dir.is_null() {
            return None;
        }
        let mid = get_method(env, get_class(env, dir), c"getAbsolutePath".as_ptr(), c"()Ljava/lang/String;".as_ptr());
        let path = call_object(env, dir, mid);
        if path.is_null() {
            return None;
        }
        let chars = utf_chars(env, path, std::ptr::null_mut());
        let s = CStr::from_ptr(chars).to_string_lossy().into_owned();
        release_chars(env, path, chars);
        Some(PathBuf::from(s))
    }
}
