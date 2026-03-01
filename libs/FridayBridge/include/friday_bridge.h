#ifndef FRIDAY_BRIDGE_H
#define FRIDAY_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

char *friday_bridge_jskit_eval(const char *source);
char *friday_bridge_corefriday_snapshot_counts(const char *database_path);
void friday_bridge_string_free(char *pointer);

#ifdef __cplusplus
}
#endif

#endif
