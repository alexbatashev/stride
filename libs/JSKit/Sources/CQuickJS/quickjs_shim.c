#include "quickjs_shim.h"

#include <quickjs.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct WrappedRuntime {
    JSRuntime *raw;
} WrappedRuntime;

typedef struct WrappedContext {
    JSContext *raw;
} WrappedContext;

typedef struct WrappedValue {
    JSValue raw;
} WrappedValue;

static JSValue qjs_console_log(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    (void)this_val;

    for (int i = 0; i < argc; i++) {
        const char *text = JS_ToCString(ctx, argv[i]);
        if (text == NULL) {
            return JS_EXCEPTION;
        }

        if (i > 0) {
            fputs(" ", stdout);
        }
        fputs(text, stdout);
        JS_FreeCString(ctx, text);
    }

    fputc('\n', stdout);
    fflush(stdout);
    return JS_UNDEFINED;
}

static int qjs_install_console(JSContext *ctx) {
    JSValue global = JS_GetGlobalObject(ctx);
    JSValue console = JS_NewObject(ctx);

    if (JS_IsException(global) || JS_IsException(console)) {
        JS_FreeValue(ctx, console);
        JS_FreeValue(ctx, global);
        return -1;
    }

    JS_SetPropertyStr(ctx, console, "log", JS_NewCFunction(ctx, qjs_console_log, "log", 1));
    JS_SetPropertyStr(ctx, console, "info", JS_NewCFunction(ctx, qjs_console_log, "info", 1));
    JS_SetPropertyStr(ctx, console, "warn", JS_NewCFunction(ctx, qjs_console_log, "warn", 1));
    JS_SetPropertyStr(ctx, console, "error", JS_NewCFunction(ctx, qjs_console_log, "error", 1));
    JS_SetPropertyStr(ctx, global, "console", console);
    JS_FreeValue(ctx, global);
    return 0;
}

QJSRuntimeRef qjs_runtime_new(void) {
    JSRuntime *runtime = JS_NewRuntime();
    if (runtime == NULL) {
        return NULL;
    }

    WrappedRuntime *wrapped = (WrappedRuntime *)malloc(sizeof(WrappedRuntime));
    if (wrapped == NULL) {
        JS_FreeRuntime(runtime);
        return NULL;
    }

    wrapped->raw = runtime;
    return (QJSRuntimeRef)wrapped;
}

void qjs_runtime_free(QJSRuntimeRef runtime_ref) {
    WrappedRuntime *runtime = (WrappedRuntime *)runtime_ref;
    if (runtime == NULL) {
        return;
    }

    JS_FreeRuntime(runtime->raw);
    free(runtime);
}

QJSContextRef qjs_context_new(QJSRuntimeRef runtime_ref) {
    WrappedRuntime *runtime = (WrappedRuntime *)runtime_ref;
    if (runtime == NULL) {
        return NULL;
    }

    JSContext *context = JS_NewContext(runtime->raw);
    if (context == NULL) {
        return NULL;
    }

    WrappedContext *wrapped = (WrappedContext *)malloc(sizeof(WrappedContext));
    if (wrapped == NULL) {
        JS_FreeContext(context);
        return NULL;
    }

    if (qjs_install_console(context) != 0) {
        JS_FreeContext(context);
        free(wrapped);
        return NULL;
    }

    wrapped->raw = context;
    return (QJSContextRef)wrapped;
}

void qjs_context_free(QJSContextRef context_ref) {
    WrappedContext *context = (WrappedContext *)context_ref;
    if (context == NULL) {
        return;
    }

    JS_FreeContext(context->raw);
    free(context);
}

QJSValueRef qjs_context_eval(QJSContextRef context_ref, const char *source, const char *file_name, int32_t flags) {
    WrappedContext *context = (WrappedContext *)context_ref;
    if (context == NULL || source == NULL || file_name == NULL) {
        return NULL;
    }

    WrappedValue *wrapped = (WrappedValue *)malloc(sizeof(WrappedValue));
    if (wrapped == NULL) {
        return NULL;
    }

    int32_t resolved_flags = (flags == 0) ? JS_EVAL_TYPE_GLOBAL : flags;
    wrapped->raw = JS_Eval(context->raw, source, strlen(source), file_name, resolved_flags);
    return (QJSValueRef)wrapped;
}

int32_t qjs_value_is_exception(QJSContextRef context_ref, QJSValueRef value_ref) {
    WrappedContext *context = (WrappedContext *)context_ref;
    WrappedValue *value = (WrappedValue *)value_ref;
    if (context == NULL || value == NULL) {
        return 1;
    }

    return JS_IsException(value->raw) ? 1 : 0;
}

void qjs_value_free(QJSContextRef context_ref, QJSValueRef value_ref) {
    WrappedContext *context = (WrappedContext *)context_ref;
    WrappedValue *value = (WrappedValue *)value_ref;
    if (context == NULL || value == NULL) {
        return;
    }

    JS_FreeValue(context->raw, value->raw);
    free(value);
}

char *qjs_context_exception_to_string(QJSContextRef context_ref) {
    WrappedContext *context = (WrappedContext *)context_ref;
    if (context == NULL) {
        return NULL;
    }

    JSValue exception = JS_GetException(context->raw);
    const char *raw = JS_ToCString(context->raw, exception);
    char *copy = NULL;

    if (raw != NULL) {
        copy = strdup(raw);
        JS_FreeCString(context->raw, raw);
    }

    JS_FreeValue(context->raw, exception);
    return copy;
}

char *qjs_value_to_string(QJSContextRef context_ref, QJSValueRef value_ref) {
    WrappedContext *context = (WrappedContext *)context_ref;
    WrappedValue *value = (WrappedValue *)value_ref;
    if (context == NULL || value == NULL) {
        return NULL;
    }

    const char *raw = JS_ToCString(context->raw, value->raw);
    if (raw == NULL) {
        return NULL;
    }

    char *copy = strdup(raw);
    JS_FreeCString(context->raw, raw);
    return copy;
}

void qjs_cstring_free(char *value) {
    free(value);
}
