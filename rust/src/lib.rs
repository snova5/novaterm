//! NovaTerm native core, exposed to Kotlin through JNI.
//!
//! Kotlin side: `object Native` in package `dev.snova.novaterm` with
//! `@JvmStatic external fun ...` methods (static, so the 2nd arg is jclass).

pub mod pty;
pub mod session;
pub mod term;

use session::Session;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Arc;

// ───────────── minimal hand-written JNI bindings ─────────────
// Function-table indices come from the JNI spec (jni.h, JNINativeInterface).

type JObject = *mut c_void;
type JNIEnv = *mut *const *const c_void;
type JLong = i64;
type JInt = i32;

mod idx {
    pub const DELETE_LOCAL_REF: usize = 23;
    pub const NEW_STRING_UTF: usize = 167;
    pub const GET_STRING_UTF_CHARS: usize = 169;
    pub const RELEASE_STRING_UTF_CHARS: usize = 170;
    pub const GET_ARRAY_LENGTH: usize = 171;
    pub const GET_OBJECT_ARRAY_ELEMENT: usize = 173;
    pub const NEW_INT_ARRAY: usize = 179;
    pub const GET_BYTE_ARRAY_REGION: usize = 200;
    pub const GET_INT_ARRAY_REGION: usize = 203;
    pub const SET_INT_ARRAY_REGION: usize = 211;
}

unsafe fn func(env: JNIEnv, i: usize) -> *const c_void {
    *(*env).add(i)
}

struct Jni(JNIEnv);

impl Jni {
    unsafe fn string(&self, s: JObject) -> String {
        if s.is_null() {
            return String::new();
        }
        let get: unsafe extern "C" fn(JNIEnv, JObject, *mut u8) -> *const c_char =
            std::mem::transmute(func(self.0, idx::GET_STRING_UTF_CHARS));
        let rel: unsafe extern "C" fn(JNIEnv, JObject, *const c_char) =
            std::mem::transmute(func(self.0, idx::RELEASE_STRING_UTF_CHARS));
        let p = get(self.0, s, std::ptr::null_mut());
        if p.is_null() {
            return String::new();
        }
        let out = CStr::from_ptr(p).to_string_lossy().into_owned();
        rel(self.0, s, p);
        out
    }

    unsafe fn array_len(&self, a: JObject) -> usize {
        if a.is_null() {
            return 0;
        }
        let f: unsafe extern "C" fn(JNIEnv, JObject) -> JInt = std::mem::transmute(func(self.0, idx::GET_ARRAY_LENGTH));
        f(self.0, a).max(0) as usize
    }

    unsafe fn string_array(&self, a: JObject) -> Vec<String> {
        let n = self.array_len(a);
        let get: unsafe extern "C" fn(JNIEnv, JObject, JInt) -> JObject =
            std::mem::transmute(func(self.0, idx::GET_OBJECT_ARRAY_ELEMENT));
        let del: unsafe extern "C" fn(JNIEnv, JObject) = std::mem::transmute(func(self.0, idx::DELETE_LOCAL_REF));
        (0..n)
            .map(|i| {
                let o = get(self.0, a, i as JInt);
                let s = self.string(o);
                if !o.is_null() {
                    del(self.0, o);
                }
                s
            })
            .collect()
    }

    unsafe fn bytes(&self, a: JObject) -> Vec<u8> {
        let n = self.array_len(a);
        let mut v = vec![0u8; n];
        if n > 0 {
            let f: unsafe extern "C" fn(JNIEnv, JObject, JInt, JInt, *mut i8) =
                std::mem::transmute(func(self.0, idx::GET_BYTE_ARRAY_REGION));
            f(self.0, a, 0, n as JInt, v.as_mut_ptr() as *mut i8);
        }
        v
    }

    unsafe fn ints(&self, a: JObject) -> Vec<i32> {
        let n = self.array_len(a);
        let mut v = vec![0i32; n];
        if n > 0 {
            let f: unsafe extern "C" fn(JNIEnv, JObject, JInt, JInt, *mut i32) =
                std::mem::transmute(func(self.0, idx::GET_INT_ARRAY_REGION));
            f(self.0, a, 0, n as JInt, v.as_mut_ptr());
        }
        v
    }

    unsafe fn int_array(&self, data: &[i32]) -> JObject {
        let new: unsafe extern "C" fn(JNIEnv, JInt) -> JObject = std::mem::transmute(func(self.0, idx::NEW_INT_ARRAY));
        let set: unsafe extern "C" fn(JNIEnv, JObject, JInt, JInt, *const i32) =
            std::mem::transmute(func(self.0, idx::SET_INT_ARRAY_REGION));
        let arr = new(self.0, data.len() as JInt);
        if !arr.is_null() {
            set(self.0, arr, 0, data.len() as JInt, data.as_ptr());
        }
        arr
    }

    unsafe fn new_string(&self, s: &str) -> JObject {
        // NewStringUTF expects modified UTF-8: keep BMP chars, no NULs.
        let clean: String = s.chars().filter(|&c| c != '\0' && (c as u32) < 0x10000).collect();
        let c = CString::new(clean).unwrap_or_default();
        let f: unsafe extern "C" fn(JNIEnv, *const c_char) -> JObject = std::mem::transmute(func(self.0, idx::NEW_STRING_UTF));
        f(self.0, c.as_ptr())
    }
}

// ───────────── handle helpers ─────────────

unsafe fn session<'a>(h: JLong) -> Option<&'a Arc<Session>> {
    (h as *const Arc<Session>).as_ref()
}

// ───────────── exported functions ─────────────

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_create(
    env: JNIEnv,
    _cls: JObject,
    program: JObject,
    argv: JObject,
    envp: JObject,
    cwd: JObject,
    rows: JInt,
    cols: JInt,
) -> JLong {
    let j = Jni(env);
    let program = j.string(program);
    let argv = j.string_array(argv);
    let envp = j.string_array(envp);
    let cwd = j.string(cwd);
    let rows = rows.clamp(1, 1000) as u16;
    let cols = cols.clamp(2, 1000) as u16;
    match Session::start(&program, &argv, &envp, &cwd, rows, cols) {
        Ok(s) => Box::into_raw(Box::new(s)) as JLong,
        Err(_) => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_write(env: JNIEnv, _cls: JObject, h: JLong, data: JObject) {
    if let Some(s) = session(h) {
        let bytes = Jni(env).bytes(data);
        s.write(&bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_feedLocal(env: JNIEnv, _cls: JObject, h: JLong, data: JObject) {
    if let Some(s) = session(h) {
        let bytes = Jni(env).bytes(data);
        s.feed_local(&bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_resize(_env: JNIEnv, _cls: JObject, h: JLong, rows: JInt, cols: JInt) {
    if let Some(s) = session(h) {
        s.resize(rows.clamp(1, 1000) as u16, cols.clamp(2, 1000) as u16);
    }
}

/// Blocks (call from a background thread). -1 = redraw, >=0 = exit code.
#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_waitUpdate(_env: JNIEnv, _cls: JObject, h: JLong) -> JInt {
    match session(h) {
        Some(s) => {
            let s = Arc::clone(s);
            s.wait_update()
        }
        None => 0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_snapshot(env: JNIEnv, _cls: JObject, h: JLong, scroll: JInt) -> JObject {
    let data = match session(h) {
        Some(s) => s.term.lock().unwrap().snapshot(scroll.max(0) as usize),
        None => Vec::new(),
    };
    Jni(env).int_array(&data)
}

/// colors = [fg, bg, cursor, ansi0..ansi15] as RGB ints.
#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_setTheme(env: JNIEnv, _cls: JObject, h: JLong, colors: JObject) {
    if let Some(s) = session(h) {
        if let Some(theme) = term::Theme::from_ints(&Jni(env).ints(colors)) {
            s.set_theme(theme);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_title(env: JNIEnv, _cls: JObject, h: JLong) -> JObject {
    let t = match session(h) {
        Some(s) => s.term.lock().unwrap().title.clone(),
        None => String::new(),
    };
    Jni(env).new_string(&t)
}

#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_hangup(_env: JNIEnv, _cls: JObject, h: JLong) {
    if let Some(s) = session(h) {
        s.hangup();
    }
}

/// Release the handle. Only call once the process has exited and no
/// thread is inside waitUpdate for this handle.
#[no_mangle]
pub unsafe extern "C" fn Java_dev_snova_novaterm_Native_free(_env: JNIEnv, _cls: JObject, h: JLong) {
    if h != 0 {
        drop(Box::from_raw(h as *mut Arc<Session>));
    }
}
