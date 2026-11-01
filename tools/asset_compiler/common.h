#ifndef ASSETC_COMMON_H
#define ASSETC_COMMON_H

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#ifdef SMG_ASSETC_MEMORY_FS
#include <stdio.h>
#endif

void die(const char *fmt, ...) __attribute__((noreturn, format(printf, 1, 2)));
void *xmalloc(size_t n);
void *xcalloc(size_t n, size_t size);
void *xrealloc(void *p, size_t n);
char *xstrdup(const char *s);
char *format_string(const char *fmt, ...) __attribute__((format(printf, 1, 2)));

uint8_t *read_file(const char *path, size_t *len);
void write_file(const char *path, const void *data, size_t len);

#ifdef SMG_ASSETC_MEMORY_FS
int smg_assetc_fprintf(FILE *stream, const char *fmt, ...);
void smg_assetc_fail(const char *message);
#endif

typedef struct {
    void *data;
    size_t len, cap, elem;
} Vector;

#define VEC(type) ((Vector){ NULL, 0, 0, sizeof(type) })
void *vec_push(Vector *v);
void vec_reserve(Vector *v, size_t n);
#define VEC_AT(v, type, i) (((type *)(v).data)[i])
void vec_free(Vector *v);

typedef struct {
    uint8_t *data;
    size_t len, cap;
} ByteBuffer;

void buffer_append(ByteBuffer *b, const void *data, size_t len);
uint32_t buffer_align(ByteBuffer *b, size_t align);
uint32_t buffer_append_aligned(ByteBuffer *b, const void *data, size_t len);

int32_t float_to_i32_saturated(float x);
int16_t float_to_i16_saturated(float x);
uint32_t float_to_u32_saturated(float x);
uint8_t double_to_u8_saturated(double x);

static inline int32_t float_to_fixed(float x, int frac) {
    return float_to_i32_saturated(x * (float)(1 << frac));
}

void stable_sort(void *base, size_t n, size_t size,
                 int (*cmp)(const void *, const void *, void *), void *ctx);

#endif
