#include "common.h"
#ifdef SMG_ASSETC_MEMORY_FS
#include "wasm_vfs.h"
#endif

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

void die(const char *fmt, ...) {
#ifdef SMG_ASSETC_MEMORY_FS
    char message[512];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(message, sizeof message, fmt, ap);
    va_end(ap);
    smg_assetc_fail(message);
#else
    va_list ap;
    va_start(ap, fmt);
    fputs("assetc: ", stderr);
    vfprintf(stderr, fmt, ap);
    fputc('\n', stderr);
    va_end(ap);
    exit(1);
#endif
}

#ifdef SMG_ASSETC_MEMORY_FS
int printf(const char *fmt, ...) {
    (void)fmt;
    return 0;
}

int smg_assetc_fprintf(FILE *stream, const char *fmt, ...) {
    (void)stream;
    (void)fmt;
    return 0;
}
#endif

void *xmalloc(size_t n) {
    void *p = malloc(n ? n : 1);
    if (!p) die("out of memory");
    return p;
}

void *xcalloc(size_t n, size_t size) {
    void *p = calloc(n ? n : 1, size ? size : 1);
    if (!p) die("out of memory");
    return p;
}

void *xrealloc(void *p, size_t n) {
    p = realloc(p, n ? n : 1);
    if (!p) die("out of memory");
    return p;
}

char *xstrdup(const char *s) {
    size_t n = strlen(s) + 1;
    return memcpy(xmalloc(n), s, n);
}

char *format_string(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    char *s = xmalloc((size_t)n + 1);
    va_start(ap, fmt);
    vsnprintf(s, (size_t)n + 1, fmt, ap);
    va_end(ap);
    return s;
}

uint8_t *read_file(const char *path, size_t *len) {
#ifdef SMG_ASSETC_MEMORY_FS
    return smg_assetc_vfs_read(path, len);
#else
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *data = xmalloc((size_t)n + 1);
    if (fread(data, 1, (size_t)n, f) != (size_t)n) die("cannot read %s", path);
    fclose(f);
    data[n] = 0;
    if (len) *len = (size_t)n;
    return data;
#endif
}

void write_file(const char *path, const void *data, size_t len) {
#ifdef SMG_ASSETC_MEMORY_FS
    smg_assetc_vfs_write(path, data, len);
#else
    FILE *f = fopen(path, "wb");
    if (!f) die("cannot write %s", path);
    if (fwrite(data, 1, len, f) != len) die("cannot write %s", path);
    fclose(f);
#endif
}

void vec_reserve(Vector *v, size_t n) {
    if (n <= v->cap) return;
    size_t cap = v->cap ? v->cap : 16;
    while (cap < n) cap *= 2;
    v->data = xrealloc(v->data, cap * v->elem);
    v->cap = cap;
}

void *vec_push(Vector *v) {
    vec_reserve(v, v->len + 1);
    void *p = (uint8_t *)v->data + v->len++ * v->elem;
    memset(p, 0, v->elem);
    return p;
}

void vec_free(Vector *v) {
    free(v->data);
    v->data = NULL;
    v->len = v->cap = 0;
}

void buffer_append(ByteBuffer *b, const void *data, size_t len) {
    if (b->len + len > b->cap) {
        size_t cap = b->cap ? b->cap : 4096;
        while (cap < b->len + len) cap *= 2;
        b->data = xrealloc(b->data, cap);
        b->cap = cap;
    }
    if (data) memcpy(b->data + b->len, data, len);
    else memset(b->data + b->len, 0, len);
    b->len += len;
}

uint32_t buffer_align(ByteBuffer *b, size_t align) {
    while (b->len % align) buffer_append(b, "", 1);
    return (uint32_t)b->len;
}

uint32_t buffer_append_aligned(ByteBuffer *b, const void *data, size_t len) {
    uint32_t off = buffer_align(b, 4);
    buffer_append(b, data, len);
    return off;
}

int32_t float_to_i32_saturated(float x) {
    if (isnan(x)) return 0;
    if (x >= 2147483648.0f) return INT32_MAX;
    if (x <= -2147483648.0f) return INT32_MIN;
    return (int32_t)x;
}

int16_t float_to_i16_saturated(float x) {
    if (isnan(x)) return 0;
    if (x >= 32767.0f) return INT16_MAX;
    if (x <= -32768.0f) return INT16_MIN;
    return (int16_t)x;
}

uint32_t float_to_u32_saturated(float x) {
    if (isnan(x) || x <= 0.0f) return 0;
    if (x >= 4294967296.0f) return UINT32_MAX;
    return (uint32_t)x;
}

uint8_t double_to_u8_saturated(double x) {
    if (isnan(x) || x <= 0.0) return 0;
    if (x >= 255.0) return 255;
    return (uint8_t)x;
}

static void merge_sort(uint8_t *a, uint8_t *tmp, size_t n, size_t size,
                       int (*cmp)(const void *, const void *, void *), void *ctx) {
    if (n < 2) return;
    size_t h = n / 2;
    merge_sort(a, tmp, h, size, cmp, ctx);
    merge_sort(a + h * size, tmp, n - h, size, cmp, ctx);
    size_t i = 0, j = h, k = 0;
    while (i < h && j < n) {
        if (cmp(a + j * size, a + i * size, ctx) < 0)
            memcpy(tmp + k++ * size, a + j++ * size, size);
        else
            memcpy(tmp + k++ * size, a + i++ * size, size);
    }
    while (i < h) memcpy(tmp + k++ * size, a + i++ * size, size);
    while (j < n) memcpy(tmp + k++ * size, a + j++ * size, size);
    memcpy(a, tmp, n * size);
}

void stable_sort(void *base, size_t n, size_t size,
                 int (*cmp)(const void *, const void *, void *), void *ctx) {
    uint8_t *tmp = xmalloc(n * size);
    merge_sort(base, tmp, n, size, cmp, ctx);
    free(tmp);
}
