#include <stdlib.h>

typedef struct {
    unsigned int abi_version;
    const char* name;
    const char* version;
} PluginInfo;

typedef char* (*PluginFn)(const char*);

// Returns NULL (no change)
char* hook_post_render(const char* input_json) {
    return NULL;
}

void norgolith_plugin_init(PluginInfo* info, unsigned int* hook_mask, PluginFn hooks[4]) {
    info->abi_version = 1;
    info->name = "test-null";
    info->version = "0.1.0";
    *hook_mask = 4; // POST_RENDER
    hooks[2] = hook_post_render;
}
