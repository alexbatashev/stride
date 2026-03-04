use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JSError {
    #[error("failed to create JavaScript runtime")]
    RuntimeCreationFailed,
    #[error("failed to create JavaScript context")]
    ContextCreationFailed,
    #[error("JavaScript evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("failed to convert JavaScript value to string")]
    StringConversionFailed,
    #[error("failed to configure JavaScript timeout")]
    TimeoutConfigurationFailed,
    #[error("source contains interior NUL byte")]
    InvalidSource,
    #[error("file name contains interior NUL byte")]
    InvalidFileName,
}

unsafe extern "C" {
    fn qjs_runtime_new() -> *mut c_void;
    fn qjs_runtime_free(runtime: *mut c_void);

    fn qjs_context_new(runtime: *mut c_void) -> *mut c_void;
    fn qjs_context_free(context: *mut c_void);

    fn qjs_context_eval(
        context: *mut c_void,
        source: *const c_char,
        file_name: *const c_char,
        flags: c_int,
    ) -> *mut c_void;
    fn qjs_value_is_exception(context: *mut c_void, value: *mut c_void) -> c_int;
    fn qjs_value_free(context: *mut c_void, value: *mut c_void);
    fn qjs_context_set_timeout(context: *mut c_void, timeout_seconds: c_int) -> c_int;

    fn qjs_context_exception_to_string(context: *mut c_void) -> *mut c_char;
    fn qjs_value_to_string(context: *mut c_void, value: *mut c_void) -> *mut c_char;
    fn qjs_context_consume_console_output(context: *mut c_void) -> *mut c_char;
    fn qjs_cstring_free(value: *mut c_char);
}

pub struct JavaScriptRuntime {
    raw_runtime: *mut c_void,
}

unsafe impl Send for JavaScriptRuntime {}

impl JavaScriptRuntime {
    pub fn new() -> Result<Self, JSError> {
        // SAFETY: FFI constructor has no preconditions.
        let raw_runtime = unsafe { qjs_runtime_new() };
        if raw_runtime.is_null() {
            return Err(JSError::RuntimeCreationFailed);
        }
        Ok(Self { raw_runtime })
    }

    pub fn make_context(&self) -> Result<JavaScriptContext, JSError> {
        // SAFETY: runtime pointer is valid for self lifetime.
        let raw_context = unsafe { qjs_context_new(self.raw_runtime) };
        if raw_context.is_null() {
            return Err(JSError::ContextCreationFailed);
        }
        Ok(JavaScriptContext { raw_context })
    }
}

impl Drop for JavaScriptRuntime {
    fn drop(&mut self) {
        // SAFETY: raw_runtime was created by qjs_runtime_new and is dropped once here.
        unsafe { qjs_runtime_free(self.raw_runtime) };
    }
}

pub struct JavaScriptContext {
    raw_context: *mut c_void,
}

unsafe impl Send for JavaScriptContext {}

impl JavaScriptContext {
    pub fn evaluate(
        &self,
        source: &str,
        file_name: &str,
        flags: i32,
        timeout_seconds: Option<i32>,
    ) -> Result<JavaScriptValue<'_>, JSError> {
        if let Some(timeout_seconds) = timeout_seconds {
            // SAFETY: raw_context is valid for self lifetime.
            if unsafe { qjs_context_set_timeout(self.raw_context, timeout_seconds as c_int) } != 0 {
                return Err(JSError::TimeoutConfigurationFailed);
            }
        } else {
            // SAFETY: raw_context is valid for self lifetime.
            let _ = unsafe { qjs_context_set_timeout(self.raw_context, 0) };
        }

        let source = CString::new(source).map_err(|_| JSError::InvalidSource)?;
        let file_name = CString::new(file_name).map_err(|_| JSError::InvalidFileName)?;

        // SAFETY: all pointers are valid and NUL-terminated for call duration.
        let raw_value = unsafe {
            qjs_context_eval(self.raw_context, source.as_ptr(), file_name.as_ptr(), flags)
        };

        // SAFETY: reset timeout for context regardless of result.
        let _ = unsafe { qjs_context_set_timeout(self.raw_context, 0) };

        if raw_value.is_null() {
            return Err(JSError::ContextCreationFailed);
        }

        // SAFETY: pointers are valid.
        if unsafe { qjs_value_is_exception(self.raw_context, raw_value) } != 0 {
            // SAFETY: pointer owned by C API and must be released by qjs_cstring_free.
            let exception = unsafe { qjs_context_exception_to_string(self.raw_context) };
            let message = cstring_to_string(exception)
                .unwrap_or_else(|| "Unknown QuickJS exception".to_owned());
            // SAFETY: result value must be released after exception check.
            unsafe { qjs_value_free(self.raw_context, raw_value) };
            return Err(JSError::EvaluationFailed(message));
        }

        Ok(JavaScriptValue {
            context: self,
            raw_value,
        })
    }

    pub fn consume_console_output(&self) -> String {
        // SAFETY: raw_context is valid.
        let out = unsafe { qjs_context_consume_console_output(self.raw_context) };
        cstring_to_string(out).unwrap_or_default()
    }
}

impl Drop for JavaScriptContext {
    fn drop(&mut self) {
        // SAFETY: raw_context was created by qjs_context_new and is dropped once.
        unsafe { qjs_context_free(self.raw_context) };
    }
}

pub struct JavaScriptValue<'ctx> {
    context: &'ctx JavaScriptContext,
    raw_value: *mut c_void,
}

impl JavaScriptValue<'_> {
    pub fn string(&self) -> Result<String, JSError> {
        // SAFETY: pointers are valid for conversion call.
        let value = unsafe { qjs_value_to_string(self.context.raw_context, self.raw_value) };
        cstring_to_string(value).ok_or(JSError::StringConversionFailed)
    }
}

impl Drop for JavaScriptValue<'_> {
    fn drop(&mut self) {
        // SAFETY: raw_value belongs to this context and is dropped once.
        unsafe { qjs_value_free(self.context.raw_context, self.raw_value) };
    }
}

fn cstring_to_string(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: pointer comes from C API and is NUL-terminated.
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: pointer must be freed with qjs_cstring_free.
    unsafe { qjs_cstring_free(ptr) };
    Some(text)
}
