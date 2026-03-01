#include <jni.h>

#include "friday_bridge.h"

JNIEXPORT jstring JNICALL
Java_me_batashev_friday_bridge_FridayBridgeBindings_nativeEvaluateJs(JNIEnv *env, jclass clazz, jstring source) {
    (void)clazz;
    if (source == NULL) {
        return (*env)->NewStringUTF(env, "Input source is null");
    }

    const char *source_chars = (*env)->GetStringUTFChars(env, source, NULL);
    char *result_chars = friday_bridge_jskit_eval(source_chars);
    (*env)->ReleaseStringUTFChars(env, source, source_chars);

    if (result_chars == NULL) {
        return (*env)->NewStringUTF(env, "JSKit bridge returned null");
    }

    jstring result = (*env)->NewStringUTF(env, result_chars);
    friday_bridge_string_free(result_chars);
    return result;
}

JNIEXPORT jstring JNICALL
Java_me_batashev_friday_bridge_FridayBridgeBindings_nativeLoadSnapshotCounts(JNIEnv *env, jclass clazz, jstring database_path) {
    (void)clazz;
    if (database_path == NULL) {
        return (*env)->NewStringUTF(env, "0,0");
    }

    const char *db_path_chars = (*env)->GetStringUTFChars(env, database_path, NULL);
    char *result_chars = friday_bridge_corefriday_snapshot_counts(db_path_chars);
    (*env)->ReleaseStringUTFChars(env, database_path, db_path_chars);

    if (result_chars == NULL) {
        return (*env)->NewStringUTF(env, "0,0");
    }

    jstring result = (*env)->NewStringUTF(env, result_chars);
    friday_bridge_string_free(result_chars);
    return result;
}
