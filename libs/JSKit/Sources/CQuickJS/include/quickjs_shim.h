#ifndef QUICKJS_SHIM_H
#define QUICKJS_SHIM_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void *QJSRuntimeRef;
typedef void *QJSContextRef;
typedef void *QJSValueRef;

QJSRuntimeRef qjs_runtime_new(void);
void qjs_runtime_free(QJSRuntimeRef runtime);

QJSContextRef qjs_context_new(QJSRuntimeRef runtime);
void qjs_context_free(QJSContextRef context);

QJSValueRef qjs_context_eval(QJSContextRef context, const char *source, const char *file_name, int32_t flags);
int32_t qjs_value_is_exception(QJSContextRef context, QJSValueRef value);
void qjs_value_free(QJSContextRef context, QJSValueRef value);

char *qjs_context_exception_to_string(QJSContextRef context);
char *qjs_value_to_string(QJSContextRef context, QJSValueRef value);
void qjs_cstring_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
