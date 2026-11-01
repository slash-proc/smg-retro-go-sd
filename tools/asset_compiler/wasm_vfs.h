#ifndef ASSETC_WASM_VFS_H
#define ASSETC_WASM_VFS_H

#include <stddef.h>
#include <stdint.h>

void smg_assetc_vfs_reset(void);
int smg_assetc_vfs_add_input(const char *name, const uint8_t *data, size_t len);
int smg_assetc_run(void);
const char *smg_assetc_error(void);
size_t smg_assetc_output_count(void);
const char *smg_assetc_output_name(size_t index);
const uint8_t *smg_assetc_output_data(size_t index);
size_t smg_assetc_output_len(size_t index);
void smg_assetc_vfs_release_output_data(size_t index);

uint8_t *smg_assetc_vfs_read(const char *path, size_t *len);
void smg_assetc_vfs_write(const char *path, const void *data, size_t len);
void smg_assetc_fail(const char *message) __attribute__((noreturn));

#endif
