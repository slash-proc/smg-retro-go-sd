#include "wasm_vfs.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wasi/wasip1.h>

typedef struct {
    char *name;
    const uint8_t *data;
    size_t len;
} InputFile;

typedef struct {
    char *name;
    uint8_t *data;
    size_t len;
} OutputFile;

extern int smg_assetc_entry(int argc, char **argv);

static InputFile *inputs;
static size_t input_count, input_capacity;
static uint32_t *input_slots;
static size_t input_slot_capacity;
static OutputFile *outputs;
static size_t output_count, output_capacity;
static char error_message[512];

static char *copy_string(const char *text) {
    size_t len = strlen(text);
    char *copy = malloc(len + 1);
    if (!copy) return NULL;
    memcpy(copy, text, len + 1);
    return copy;
}

static const char *strip_root(const char *path, const char *root) {
    size_t len = strlen(root);
    if (strncmp(path, root, len) || path[len] != '/') return NULL;
    return path + len + 1;
}

static uint32_t name_hash(const char *name) {
    uint32_t hash = 2166136261u;
    for (const unsigned char *p = (const unsigned char *)name; *p; p++) {
        hash ^= *p;
        hash *= 16777619u;
    }
    return hash;
}

static size_t input_find(const char *name) {
    if (!input_slot_capacity) return input_count;
    size_t mask = input_slot_capacity - 1;
    size_t slot = name_hash(name) & mask;
    while (input_slots[slot]) {
        size_t index = (size_t)input_slots[slot] - 1;
        if (!strcmp(inputs[index].name, name)) return index;
        slot = (slot + 1) & mask;
    }
    return input_count;
}

static int reserve_input_index(size_t wanted) {
    if (input_slot_capacity && wanted * 10 < input_slot_capacity * 7) return 1;
    size_t capacity = input_slot_capacity ? input_slot_capacity * 2 : 256;
    while (wanted * 10 >= capacity * 7) capacity *= 2;
    uint32_t *slots = calloc(capacity, sizeof *slots);
    if (!slots) return 0;
    size_t mask = capacity - 1;
    for (size_t index = 0; index < input_count; index++) {
        size_t slot = name_hash(inputs[index].name) & mask;
        while (slots[slot]) slot = (slot + 1) & mask;
        slots[slot] = (uint32_t)index + 1;
    }
    free(input_slots);
    input_slots = slots;
    input_slot_capacity = capacity;
    return 1;
}

static void input_index_insert(size_t index) {
    size_t mask = input_slot_capacity - 1;
    size_t slot = name_hash(inputs[index].name) & mask;
    while (input_slots[slot]) slot = (slot + 1) & mask;
    input_slots[slot] = (uint32_t)index + 1;
}

static int reserve_inputs(size_t count) {
    if (count <= input_capacity) return 1;
    size_t capacity = input_capacity ? input_capacity * 2 : 128;
    while (capacity < count) capacity *= 2;
    InputFile *next = realloc(inputs, capacity * sizeof *inputs);
    if (!next) return 0;
    inputs = next;
    input_capacity = capacity;
    return 1;
}

static int reserve_outputs(size_t count) {
    if (count <= output_capacity) return 1;
    size_t capacity = output_capacity ? output_capacity * 2 : 128;
    while (capacity < count) capacity *= 2;
    OutputFile *next = realloc(outputs, capacity * sizeof *outputs);
    if (!next) return 0;
    outputs = next;
    output_capacity = capacity;
    return 1;
}

void smg_assetc_vfs_reset(void) {
    for (size_t i = 0; i < input_count; i++) free(inputs[i].name);
    for (size_t i = 0; i < output_count; i++) {
        free(outputs[i].name);
        free(outputs[i].data);
    }
    free(inputs);
    free(input_slots);
    free(outputs);
    inputs = NULL;
    input_slots = NULL;
    outputs = NULL;
    input_count = input_capacity = 0;
    input_slot_capacity = 0;
    output_count = output_capacity = 0;
    error_message[0] = 0;
}

int smg_assetc_vfs_add_input(const char *name, const uint8_t *data, size_t len) {
    if (!name || (!data && len)) return 0;
    size_t found = input_find(name);
    if (found < input_count) {
        inputs[found].data = data;
        inputs[found].len = len;
        return 1;
    }
    if (!reserve_input_index(input_count + 1) || !reserve_inputs(input_count + 1)) return 0;
    char *copy = copy_string(name);
    if (!copy) return 0;
    size_t index = input_count++;
    inputs[index] = (InputFile){ .name = copy, .data = data, .len = len };
    input_index_insert(index);
    return 1;
}

uint8_t *smg_assetc_vfs_read(const char *path, size_t *len) {
    const char *name = strip_root(path, "input");
    if (!name) return NULL;
    size_t index = input_find(name);
    if (index == input_count) return NULL;
    uint8_t *copy = malloc(inputs[index].len + 1);
    if (!copy) smg_assetc_fail("asset compiler input allocation failed");
    memcpy(copy, inputs[index].data, inputs[index].len);
    copy[inputs[index].len] = 0;
    if (len) *len = inputs[index].len;
    return copy;
}

void smg_assetc_vfs_write(const char *path, const void *data, size_t len) {
    const char *name = strip_root(path, "output");
    if (!name || (!data && len)) smg_assetc_fail("asset compiler wrote an invalid path");

    size_t index = 0;
    while (index < output_count && strcmp(outputs[index].name, name)) index++;
    char *copy_name = index < output_count ? NULL : copy_string(name);
    uint8_t *copy_data = malloc(len ? len : 1);
    if ((!copy_name && index == output_count) || !copy_data)
        smg_assetc_fail("asset compiler output allocation failed");
    if (len) memcpy(copy_data, data, len);

    if (index == output_count) {
        if (!reserve_outputs(output_count + 1)) {
            free(copy_name);
            free(copy_data);
            smg_assetc_fail("asset compiler output index allocation failed");
        }
        outputs[output_count++] = (OutputFile){ .name = copy_name, .data = copy_data, .len = len };
    } else {
        free(outputs[index].data);
        outputs[index].data = copy_data;
        outputs[index].len = len;
    }
}

void smg_assetc_fail(const char *message) {
    if (!message) message = "asset compiler failed";
    snprintf(error_message, sizeof error_message, "%s", message);
    __builtin_trap();
}

const char *smg_assetc_error(void) { return error_message; }

int smg_assetc_run(void) {
    char program[] = "assetc";
    char input[] = "input";
    char output[] = "output";
    char *argv[] = { program, input, output, NULL };
    return smg_assetc_entry(3, argv);
}

size_t smg_assetc_output_count(void) { return output_count; }
const char *smg_assetc_output_name(size_t index) {
    return index < output_count ? outputs[index].name : NULL;
}
const uint8_t *smg_assetc_output_data(size_t index) {
    return index < output_count ? outputs[index].data : NULL;
}
size_t smg_assetc_output_len(size_t index) {
    return index < output_count ? outputs[index].len : 0;
}

void smg_assetc_vfs_release_output_data(size_t index) {
    if (index >= output_count) return;
    free(outputs[index].data);
    outputs[index].data = NULL;
    outputs[index].len = 0;
}

#ifdef SMG_ASSETC_MEMORY_FS
/* WASI libc keeps these imports for stdio setup and teardown even though the
 * in-memory asset compiler redirects its own file access above. Keep the GWRG
 * module import-free and provide the inert console/runtime operations here. */
__wasi_errno_t __wasi_clock_time_get(__wasi_clockid_t id, __wasi_timestamp_t precision,
                                     __wasi_timestamp_t *out) {
    (void)id;
    (void)precision;
    *out = 0;
    return 0;
}

__wasi_errno_t __wasi_fd_close(__wasi_fd_t fd) {
    (void)fd;
    return 0;
}

__wasi_errno_t __wasi_fd_fdstat_get(__wasi_fd_t fd, __wasi_fdstat_t *out) {
    (void)fd;
    memset(out, 0, sizeof *out);
    out->fs_filetype = __WASI_FILETYPE_CHARACTER_DEVICE;
    return 0;
}

__wasi_errno_t __wasi_fd_seek(__wasi_fd_t fd, __wasi_filedelta_t offset,
                              __wasi_whence_t whence, __wasi_filesize_t *out) {
    (void)fd;
    (void)offset;
    (void)whence;
    *out = 0;
    return 8;
}

__wasi_errno_t __wasi_fd_write(__wasi_fd_t fd, const __wasi_ciovec_t *iovs, size_t count,
                               __wasi_size_t *out) {
    (void)fd;
    uint64_t discarded = 0;
    for (size_t i = 0; i < count; i++) discarded += iovs[i].buf_len;
    *out = discarded > UINT32_MAX ? UINT32_MAX : (__wasi_size_t)discarded;
    return 0;
}
#endif
