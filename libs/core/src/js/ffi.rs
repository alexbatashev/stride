use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const JS_TAG_UNDEFINED: i64 = 3;
const JS_TAG_EXCEPTION: i64 = 6;
const JS_EVAL_TYPE_GLOBAL: c_int = 0;
const JS_CFUNC_GENERIC: c_int = 0;

#[repr(C)]
struct JSRuntime {
    _private: [u8; 0],
}

#[repr(C)]
struct JSContext {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
union JSValueUnion {
    int32_: i32,
    float64_: f64,
    ptr_: *mut c_void,
    short_big_int_: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct JSValue {
    u: JSValueUnion,
    tag: i64,
}

type JSValueConst = JSValue;

type JSCFunction = unsafe extern "C" fn(
    ctx: *mut JSContext,
    this_val: JSValueConst,
    argc: c_int,
    argv: *mut JSValueConst,
) -> JSValue;

type JSInterruptHandler = unsafe extern "C" fn(rt: *mut JSRuntime, opaque: *mut c_void) -> c_int;

unsafe extern "C" {
    fn JS_NewRuntime() -> *mut JSRuntime;
    fn JS_FreeRuntime(rt: *mut JSRuntime);
    fn JS_SetInterruptHandler(
        rt: *mut JSRuntime,
        cb: Option<JSInterruptHandler>,
        opaque: *mut c_void,
    );

    fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext;
    fn JS_FreeContext(ctx: *mut JSContext);
    fn JS_GetContextOpaque(ctx: *mut JSContext) -> *mut c_void;
    fn JS_SetContextOpaque(ctx: *mut JSContext, opaque: *mut c_void);

    fn JS_Eval(
        ctx: *mut JSContext,
        input: *const c_char,
        input_len: usize,
        filename: *const c_char,
        flags: c_int,
    ) -> JSValue;
    fn JS_GetException(ctx: *mut JSContext) -> JSValue;
    fn JS_FreeValue(ctx: *mut JSContext, value: JSValue);
    fn JS_ToCStringLen2(
        ctx: *mut JSContext,
        plen: *mut usize,
        value: JSValueConst,
        cesu8: bool,
    ) -> *const c_char;
    fn JS_FreeCString(ctx: *mut JSContext, ptr: *const c_char);

    fn JS_GetGlobalObject(ctx: *mut JSContext) -> JSValue;
    fn JS_NewObject(ctx: *mut JSContext) -> JSValue;
    fn JS_NewCFunction2(
        ctx: *mut JSContext,
        func: Option<JSCFunction>,
        name: *const c_char,
        length: c_int,
        cproto: c_int,
        magic: c_int,
    ) -> JSValue;
    fn JS_SetPropertyStr(
        ctx: *mut JSContext,
        this_obj: JSValueConst,
        prop: *const c_char,
        value: JSValue,
    ) -> c_int;
}

struct RuntimeHandle {
    raw: *mut JSRuntime,
    start: Instant,
    deadline_ms: AtomicU64,
}

struct ContextHandle {
    raw: *mut JSContext,
    runtime: *mut RuntimeHandle,
    console_output: Mutex<String>,
}

#[derive(Copy, Clone)]
struct ValueHandle {
    raw: JSValue,
}

fn js_mkval(tag: i64, value: i32) -> JSValue {
    JSValue {
        u: JSValueUnion { int32_: value },
        tag,
    }
}

fn js_undefined() -> JSValue {
    js_mkval(JS_TAG_UNDEFINED, 0)
}

fn js_exception() -> JSValue {
    js_mkval(JS_TAG_EXCEPTION, 0)
}

fn js_is_exception(value: JSValue) -> bool {
    value.tag == JS_TAG_EXCEPTION
}

unsafe extern "C" fn interrupt_handler(_rt: *mut JSRuntime, opaque: *mut c_void) -> c_int {
    if opaque.is_null() {
        return 0;
    }

    // SAFETY: opaque is set to a valid RuntimeHandle pointer while runtime is alive.
    let runtime = unsafe { &*(opaque as *mut RuntimeHandle) };
    let deadline_ms = runtime.deadline_ms.load(Ordering::Relaxed);
    if deadline_ms == 0 {
        return 0;
    }

    let now_ms = runtime.start.elapsed().as_millis() as u64;
    if now_ms >= deadline_ms { 1 } else { 0 }
}

unsafe extern "C" fn console_log(
    ctx: *mut JSContext,
    _this_val: JSValueConst,
    argc: c_int,
    argv: *mut JSValueConst,
) -> JSValue {
    if ctx.is_null() {
        return js_exception();
    }

    // SAFETY: ctx is valid for the callback invocation.
    let context_opaque = unsafe { JS_GetContextOpaque(ctx) };
    if context_opaque.is_null() {
        return js_exception();
    }

    // SAFETY: context opaque was initialized to ContextHandle in qjs_context_new.
    let context = unsafe { &*(context_opaque as *mut ContextHandle) };
    let mut out = match context.console_output.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if argc > 0 {
        for index in 0..(argc as isize) {
            if index > 0 {
                out.push(' ');
            }

            // SAFETY: argv points to argc entries per QuickJS callback contract.
            let value = unsafe { *argv.offset(index) };
            // SAFETY: ctx is valid; QuickJS returns null on conversion failure.
            let raw_text = unsafe { JS_ToCStringLen2(ctx, ptr::null_mut(), value, false) };
            if raw_text.is_null() {
                return js_exception();
            }

            // SAFETY: raw_text is a NUL-terminated string owned by QuickJS.
            let text = unsafe { CStr::from_ptr(raw_text) }.to_string_lossy();
            out.push_str(&text);

            // SAFETY: raw_text was returned by JS_ToCStringLen2.
            unsafe { JS_FreeCString(ctx, raw_text) };
        }
    }

    out.push('\n');
    js_undefined()
}

fn string_to_c_ptr(value: String) -> *mut c_char {
    CString::new(value).map_or(ptr::null_mut(), CString::into_raw)
}

fn js_to_string_copy(ctx: *mut JSContext, value: JSValue) -> *mut c_char {
    // SAFETY: ctx and value are from QuickJS APIs.
    let raw = unsafe { JS_ToCStringLen2(ctx, ptr::null_mut(), value, false) };
    if raw.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: raw is valid and NUL-terminated while held by QuickJS.
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: raw was returned by JS_ToCStringLen2 and must be released by JS_FreeCString.
    unsafe { JS_FreeCString(ctx, raw) };

    string_to_c_ptr(text)
}

fn install_console(ctx: *mut JSContext) -> c_int {
    // SAFETY: ctx is a valid JSContext pointer.
    let global = unsafe { JS_GetGlobalObject(ctx) };
    if js_is_exception(global) {
        return -1;
    }

    // SAFETY: ctx is a valid JSContext pointer.
    let console = unsafe { JS_NewObject(ctx) };
    if js_is_exception(console) {
        // SAFETY: global is a valid owned JSValue.
        unsafe { JS_FreeValue(ctx, global) };
        return -1;
    }

    // SAFETY: creating C functions bound to static callback.
    let log_fn = unsafe {
        JS_NewCFunction2(
            ctx,
            Some(console_log),
            b"log\0".as_ptr().cast(),
            1,
            JS_CFUNC_GENERIC,
            0,
        )
    };
    // SAFETY: creating C functions bound to static callback.
    let info_fn = unsafe {
        JS_NewCFunction2(
            ctx,
            Some(console_log),
            b"info\0".as_ptr().cast(),
            1,
            JS_CFUNC_GENERIC,
            0,
        )
    };
    // SAFETY: creating C functions bound to static callback.
    let warn_fn = unsafe {
        JS_NewCFunction2(
            ctx,
            Some(console_log),
            b"warn\0".as_ptr().cast(),
            1,
            JS_CFUNC_GENERIC,
            0,
        )
    };
    // SAFETY: creating C functions bound to static callback.
    let error_fn = unsafe {
        JS_NewCFunction2(
            ctx,
            Some(console_log),
            b"error\0".as_ptr().cast(),
            1,
            JS_CFUNC_GENERIC,
            0,
        )
    };

    // SAFETY: properties are attached to valid JS objects. JS_SetPropertyStr takes ownership of value.
    unsafe {
        JS_SetPropertyStr(ctx, console, b"log\0".as_ptr().cast(), log_fn);
        JS_SetPropertyStr(ctx, console, b"info\0".as_ptr().cast(), info_fn);
        JS_SetPropertyStr(ctx, console, b"warn\0".as_ptr().cast(), warn_fn);
        JS_SetPropertyStr(ctx, console, b"error\0".as_ptr().cast(), error_fn);
        JS_SetPropertyStr(ctx, global, b"console\0".as_ptr().cast(), console);
        JS_FreeValue(ctx, global);
    }

    0
}

pub(super) fn qjs_runtime_new() -> *mut c_void {
    // SAFETY: no preconditions.
    let raw = unsafe { JS_NewRuntime() };
    if raw.is_null() {
        return ptr::null_mut();
    }

    let runtime = Box::new(RuntimeHandle {
        raw,
        start: Instant::now(),
        deadline_ms: AtomicU64::new(0),
    });
    let runtime_ptr = Box::into_raw(runtime);

    // SAFETY: raw runtime and opaque pointer are valid for runtime lifetime.
    unsafe { JS_SetInterruptHandler(raw, Some(interrupt_handler), runtime_ptr.cast()) };

    runtime_ptr.cast()
}

pub(super) fn qjs_runtime_free(runtime: *mut c_void) {
    if runtime.is_null() {
        return;
    }

    // SAFETY: runtime was allocated in qjs_runtime_new.
    let runtime = unsafe { Box::from_raw(runtime as *mut RuntimeHandle) };
    // SAFETY: runtime.raw is valid and freed exactly once.
    unsafe { JS_FreeRuntime(runtime.raw) };
}

pub(super) fn qjs_context_new(runtime: *mut c_void) -> *mut c_void {
    if runtime.is_null() {
        return ptr::null_mut();
    }

    let runtime = runtime as *mut RuntimeHandle;

    // SAFETY: runtime pointer is valid.
    // SAFETY: runtime points to a valid RuntimeHandle.
    let runtime_raw = unsafe { (*runtime).raw };
    // SAFETY: runtime_raw is valid.
    let raw_context = unsafe { JS_NewContext(runtime_raw) };
    if raw_context.is_null() {
        return ptr::null_mut();
    }

    if install_console(raw_context) != 0 {
        // SAFETY: raw_context is valid and must be released on failure.
        unsafe { JS_FreeContext(raw_context) };
        return ptr::null_mut();
    }

    let context = Box::new(ContextHandle {
        raw: raw_context,
        runtime,
        console_output: Mutex::new(String::new()),
    });
    let context_ptr = Box::into_raw(context);

    // SAFETY: context pointer remains valid until qjs_context_free.
    unsafe { JS_SetContextOpaque(raw_context, context_ptr.cast()) };

    context_ptr.cast()
}

pub(super) fn qjs_context_free(context: *mut c_void) {
    if context.is_null() {
        return;
    }

    // SAFETY: context was allocated in qjs_context_new.
    let context = unsafe { Box::from_raw(context as *mut ContextHandle) };

    // SAFETY: context.raw is valid and opaque can be cleared before free.
    unsafe {
        JS_SetContextOpaque(context.raw, ptr::null_mut());
        JS_FreeContext(context.raw);
    }
}

pub(super) fn qjs_context_eval(
    context: *mut c_void,
    source: *const c_char,
    file_name: *const c_char,
    flags: c_int,
) -> *mut c_void {
    if context.is_null() || source.is_null() || file_name.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: context points to a valid ContextHandle.
    let context = unsafe { &*(context as *mut ContextHandle) };

    // SAFETY: pointers are valid and NUL-terminated for this call.
    let len = unsafe { CStr::from_ptr(source).to_bytes().len() };
    let eval_flags = if flags == 0 {
        JS_EVAL_TYPE_GLOBAL
    } else {
        flags
    };

    // SAFETY: context and pointers are valid for invocation.
    let value = unsafe { JS_Eval(context.raw, source, len, file_name, eval_flags) };
    let wrapped = Box::new(ValueHandle { raw: value });
    Box::into_raw(wrapped).cast()
}

pub(super) fn qjs_value_is_exception(context: *mut c_void, value: *mut c_void) -> c_int {
    if context.is_null() || value.is_null() {
        return 1;
    }

    // SAFETY: value points to a valid ValueHandle.
    let value = unsafe { &*(value as *mut ValueHandle) };
    if js_is_exception(value.raw) { 1 } else { 0 }
}

pub(super) fn qjs_value_free(context: *mut c_void, value: *mut c_void) {
    if context.is_null() || value.is_null() {
        return;
    }

    // SAFETY: context and value were created by qjs_context_new/qjs_context_eval.
    let context = unsafe { &*(context as *mut ContextHandle) };
    // SAFETY: value is uniquely owned and consumed here.
    let value = unsafe { Box::from_raw(value as *mut ValueHandle) };

    // SAFETY: JSValue belongs to this context and is released once.
    unsafe { JS_FreeValue(context.raw, value.raw) };
}

pub(super) fn qjs_context_set_timeout(context: *mut c_void, timeout_seconds: c_int) -> c_int {
    if context.is_null() {
        return -1;
    }

    // SAFETY: context points to a valid ContextHandle.
    let context = unsafe { &*(context as *mut ContextHandle) };
    if context.runtime.is_null() {
        return -1;
    }

    // SAFETY: runtime pointer comes from a live context.
    let runtime = unsafe { &*context.runtime };
    if timeout_seconds <= 0 {
        runtime.deadline_ms.store(0, Ordering::Relaxed);
        return 0;
    }

    let now_ms = runtime.start.elapsed().as_millis() as u64;
    let timeout_ms = (timeout_seconds as u64).saturating_mul(1000);
    runtime
        .deadline_ms
        .store(now_ms.saturating_add(timeout_ms), Ordering::Relaxed);
    0
}

pub(super) fn qjs_context_exception_to_string(context: *mut c_void) -> *mut c_char {
    if context.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: context points to a valid ContextHandle.
    let context = unsafe { &*(context as *mut ContextHandle) };
    // SAFETY: context.raw is valid.
    let exception = unsafe { JS_GetException(context.raw) };
    let copy = js_to_string_copy(context.raw, exception);
    // SAFETY: exception value was returned by JS_GetException and must be freed.
    unsafe { JS_FreeValue(context.raw, exception) };
    copy
}

pub(super) fn qjs_value_to_string(context: *mut c_void, value: *mut c_void) -> *mut c_char {
    if context.is_null() || value.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: context/value pointers are valid.
    let context = unsafe { &*(context as *mut ContextHandle) };
    // SAFETY: value points to a valid ValueHandle.
    let value = unsafe { &*(value as *mut ValueHandle) };

    js_to_string_copy(context.raw, value.raw)
}

pub(super) fn qjs_context_consume_console_output(context: *mut c_void) -> *mut c_char {
    if context.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: context points to a valid ContextHandle.
    let context = unsafe { &*(context as *mut ContextHandle) };
    let mut out = match context.console_output.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    let captured = std::mem::take(&mut *out);
    string_to_c_ptr(captured)
}

pub(super) fn qjs_cstring_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    // SAFETY: pointer was allocated with CString::into_raw in this module.
    unsafe {
        let _ = CString::from_raw(value);
    }
}
