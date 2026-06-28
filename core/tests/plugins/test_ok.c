#include <stdlib.h>
#include <string.h>

typedef struct {
    unsigned int abi_version;
    const char* name;
    const char* version;
} PluginInfo;

typedef char* (*PluginFn)(const char*);

// post_render hook: echoes input back with " [transformed]" appended to html
char* hook_post_render(const char* input_json) {
    // Find "html" field value (simple parse for testing)
    const char* html_start = strstr(input_json, "\"html\":\"");
    if (!html_start) return NULL;
    html_start += 8;

    const char* html_end = strchr(html_start, '"');
    if (!html_end) return NULL;

    size_t html_len = html_end - html_start;
    size_t suffix_len = strlen(" [transformed]");

    char* result = malloc(html_len + suffix_len + 34); // room for JSON wrapper
    memcpy(result, "{\"html\":\"", 9);
    memcpy(result + 9, html_start, html_len);
    memcpy(result + 9 + html_len, " [transformed]\"", 16);
    result[9 + html_len + 15] = '}';
    result[9 + html_len + 16] = '\0';
    return result;
}

void norgolith_plugin_init(PluginInfo* info, unsigned int* hook_mask, PluginFn hooks[4]) {
    info->abi_version = 1;
    info->name = "test-ok";
    info->version = "0.1.0";
    *hook_mask = 4; // POST_RENDER = bit 2
    hooks[2] = hook_post_render;
}
