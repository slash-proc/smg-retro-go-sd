#include <math.h>
#ifndef TEXTURES_BAKE_CORE_LIBRARY
#include <stdio.h>
#endif
#include <stdlib.h>
#include <string.h>

#ifdef TEXTURES_BAKE_CORE_LIBRARY
/* Freestanding WASM math shims stay internal to this C object; exposing them
 * from the final module would violate the GWRG processor ABI export surface. */
float sqrtf(float value) { return __builtin_sqrtf(value); }
double sqrt(double value) { return __builtin_sqrt(value); }
float floorf(float value) { return __builtin_floorf(value); }
float ceilf(float value) { return __builtin_ceilf(value); }
double floor(double value) { return __builtin_floor(value); }
double ceil(double value) { return __builtin_ceil(value); }
float fabsf(float value) { return __builtin_fabsf(value); }
long lroundf(float value)
{
    return value >= 0.0f ? (long)(value + 0.5f) : (long)(value - 0.5f);
}
#endif

#ifdef TEXTURES_BAKE_CORE_ARENA
static unsigned char *core_arena;
static size_t core_arena_size;
static size_t core_arena_used;
static int core_arena_failed;

static void *core_malloc(size_t size)
{
    size_t header = 16, start, end;
    if (!size)
        size = 1;
    start = (core_arena_used + 15) & ~(size_t)15;
    if (start > core_arena_size || header > core_arena_size - start ||
        size > core_arena_size - start - header) {
        core_arena_failed = 1;
        return NULL;
    }
    end = start + header + size;
    memcpy(core_arena + start, &size, sizeof size);
    core_arena_used = end;
    return core_arena + start + header;
}

static void *core_calloc(size_t count, size_t size)
{
    if (count && size > (size_t)-1 / count) {
        core_arena_failed = 1;
        return NULL;
    }
    size_t bytes = count * size;
    void *ptr = core_malloc(bytes);
    if (ptr)
        memset(ptr, 0, bytes);
    return ptr;
}

static void *core_realloc(void *ptr, size_t size)
{
    if (!ptr)
        return core_malloc(size);
    if (!size)
        return NULL;
    size_t old_size;
    unsigned char *p = ptr;
    memcpy(&old_size, p - 16, sizeof old_size);
    size_t block_end = (size_t)(p - core_arena) + old_size;
    if (block_end == core_arena_used && size <= core_arena_size - (size_t)(p - core_arena)) {
        memcpy(p - 16, &size, sizeof size);
        core_arena_used = (size_t)(p - core_arena) + size;
        return ptr;
    }
    void *replacement = core_malloc(size);
    if (replacement)
        memcpy(replacement, ptr, old_size < size ? old_size : size);
    return replacement;
}

static void core_free(void *ptr) { (void)ptr; }

#define malloc core_malloc
#define calloc core_calloc
#define realloc core_realloc
#define free core_free
#endif

#define FLAG_AA_TABLE 32
#define FLAG_PARTS 65536
#define FLAG_CUT 131072
#define FLAG_PART_IDS 1048576
#define FLAG_FAR 2097152
#define FLAG_REPROJECT 4194304
#define FLAG_NEAR_MAP 8388608
#define FLAG_RAYCAST 33554432
#define FLAG_POINT_NEAR 67108864

typedef struct {
    float x, y, z;
} V3;

static const unsigned char *buf;
static long bpos;
static unsigned char *result_buf;
static size_t result_len;
static size_t result_cap;
static int result_failed;

static int result_write(const void *data, size_t size)
{
    size_t needed = result_len + size;
    if (needed < result_len)
        return 0;
    if (needed > result_cap) {
        size_t cap = result_cap ? result_cap : 4096;
        while (cap < needed) {
            if (cap > (size_t)-1 / 2) {
                cap = needed;
                break;
            }
            cap *= 2;
        }
        unsigned char *p = realloc(result_buf, cap);
        if (!p) {
            result_failed = 1;
            return 0;
        }
        result_buf = p;
        result_cap = cap;
    }
    memcpy(result_buf + result_len, data, size);
    result_len = needed;
    return 1;
}
#ifndef TEXTURES_BAKE_CORE_LIBRARY
static unsigned char *owned_buf;
#endif

#ifndef TEXTURES_BAKE_CORE_LIBRARY
static const char *native_output_path;

static void load_input(const char *path)
{
    FILE *f = fopen(path, "rb");
    long n;
    fseek(f, 0, SEEK_END);
    n = ftell(f);
    fseek(f, 0, SEEK_SET);
    owned_buf = malloc(n);
    if (fread(owned_buf, 1, n, f) != (size_t)n)
        exit(2);
    fclose(f);
    buf = owned_buf;
    bpos = 0;
}
#endif

static void write_file(const char *path, const void *data, size_t size)
{
    (void)path;
    if (!result_write(data, size))
        result_failed = 1;
}

static int rd_i(void)
{
    int v;
    memcpy(&v, buf + bpos, 4);
    bpos += 4;
    return v;
}

static float rd_f(void)
{
    float v;
    memcpy(&v, buf + bpos, 4);
    bpos += 4;
    return v;
}

static double rd_f64(void)
{
    double v;
    memcpy(&v, buf + bpos, 8);
    bpos += 8;
    return v;
}

static V3 v3(float x, float y, float z)
{
    V3 r;
    r.x = x;
    r.y = y;
    r.z = z;
    return r;
}

static V3 sub(V3 a, V3 b)
{
    return v3(a.x - b.x, a.y - b.y, a.z - b.z);
}

static V3 add(V3 a, V3 b)
{
    return v3(a.x + b.x, a.y + b.y, a.z + b.z);
}

static V3 mulf(V3 a, float f)
{
    return v3(a.x * f, a.y * f, a.z * f);
}

static float dot(V3 a, V3 b)
{
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

static V3 cross(V3 a, V3 b)
{
    return v3(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
}

static V3 norm(V3 a)
{
    float d = a.x * a.x + a.y * a.y + a.z * a.z;
    if (d > 1.0e-35f) {
        d = 1.0f / sqrtf(d);
        return mulf(a, d);
    }
    return a;
}

static double pw_root(double x, int n)
{
    double r, p, nr;
    int i, k;
    if (x <= 0.0)
        return 0.0;
    r = x > 1.0 ? x : 1.0;
    for (i = 0; i < 400; i++) {
        p = 1.0;
        for (k = 0; k < n - 1; k++)
            p *= r;
        nr = ((n - 1) * r + x / p) / n;
        if (nr >= r)
            break;
        r = nr;
    }
    return r;
}

static double srgb_to_lin(double c)
{
    double t;
    if (c <= 0.04045)
        return c / 12.92;
    t = (c + 0.055) / 1.055;
    return t * t * pw_root(t * t, 5);
}

static double lin_to_srgb(double c)
{
    double r;
    if (c <= 0.0031308)
        return c * 12.92;
    r = pw_root(c, 12);
    return 1.055 * (r * r * r * r * r) - 0.055;
}

static float lut_s2l[256];
static float thr[257];
static float vq[256];

static int to_byte_lin(double c)
{
    int lo = 0, hi = 256, mid;
    while (hi - lo > 1) {
        mid = (lo + hi) >> 1;
        if (c >= thr[mid])
            lo = mid;
        else
            hi = mid;
    }
    return lo;
}

static void init_tables(void)
{
    int b;
    double lo, hi, mid, s;
    for (b = 0; b < 256; b++)
        lut_s2l[b] = (float)srgb_to_lin(b / 255.0);
    for (b = 0; b < 256; b++) {
        float sv = (float)lin_to_srgb(b / 255.0);
        int q = sv <= 0.0f ? 0 : sv > 1.0f - 0.5f / 255.0f ? 255 : (int)(255.0f * sv + 0.5f);
        vq[b] = lut_s2l[q];
    }
    thr[0] = -1e30f;
    for (b = 1; b < 256; b++) {
        lo = 0.0;
        hi = 1.0;
        for (int it = 0; it < 64; it++) {
            mid = (lo + hi) * 0.5;
            s = lin_to_srgb(mid);
            if ((float)s > 1.0f - 0.5f / 255.0f) {
                hi = mid;
                continue;
            }
            if ((int)((float)(255.0f * (float)s) + 0.5f) >= b)
                hi = mid;
            else
                lo = mid;
        }
        thr[b] = (float)hi;
    }
    thr[256] = 1e30f;
}

typedef struct {
    int tex, wrap_s, wrap_t, alpha_mode, has_xf;
    float factor[4], cutoff, xf[4];
} Mat;

typedef struct {
    int w, h;
    const unsigned char *px;
} Tex;

static int nlv, nlt, npoly, nhv, nht, nmat, ntex, W, H, MARGIN, NS, FLAGS;
static float EXT;
static V3 *lv;
static int *lt;
static float *luv;
static int *lpoly;
static int *pstart, *pcount;
static float *ploopuv;
static V3 *hv;
static float *huv, *hcol, *hnrm;
static int *ht, *hmat, *hpart;
static int cur_part = -1;
static Mat *mats;
static Tex *texs;
static float *aa_off;

typedef struct {
    float lo[3], hi[3];
    int left, right, first, count;
} Node;

static Node *nodes;
static int nnodes;
static int *tri_idx;
static float *tcent;

static int build(int first, int count)
{
    int ni = nnodes++, i, k, axis, mid;
    Node *n = &nodes[ni];
    float lo[3] = { 1e30f, 1e30f, 1e30f }, hi[3] = { -1e30f, -1e30f, -1e30f };
    float clo[3] = { 1e30f, 1e30f, 1e30f }, chi[3] = { -1e30f, -1e30f, -1e30f };
    for (i = first; i < first + count; i++) {
        int t = tri_idx[i];
        for (k = 0; k < 3; k++) {
            V3 p = hv[ht[t * 3 + k]];
            float c[3] = { p.x, p.y, p.z };
            for (int a = 0; a < 3; a++) {
                if (c[a] < lo[a])
                    lo[a] = c[a];
                if (c[a] > hi[a])
                    hi[a] = c[a];
            }
        }
        for (int a = 0; a < 3; a++) {
            float c = tcent[t * 3 + a];
            if (c < clo[a])
                clo[a] = c;
            if (c > chi[a])
                chi[a] = c;
        }
    }
    memcpy(n->lo, lo, sizeof lo);
    memcpy(n->hi, hi, sizeof hi);
    n->first = first;
    n->count = count;
    n->left = n->right = -1;
    if (count <= 4)
        return ni;
    axis = 0;
    for (int a = 1; a < 3; a++)
        if (chi[a] - clo[a] > chi[axis] - clo[axis])
            axis = a;
    float split = (clo[axis] + chi[axis]) * 0.5f;
    int l = first, r = first + count - 1;
    while (l <= r) {
        if (tcent[tri_idx[l] * 3 + axis] < split)
            l++;
        else {
            int tmp = tri_idx[l];
            tri_idx[l] = tri_idx[r];
            tri_idx[r] = tmp;
            r--;
        }
    }
    mid = l - first;
    if (mid == 0 || mid == count)
        mid = count / 2;
    int left = build(first, mid);
    int right = build(first + mid, count - mid);
    nodes[ni].left = left;
    nodes[ni].right = right;
    return ni;
}

static int box_hit(const Node *n, V3 o, V3 inv, float tmax)
{
    float t0 = 0.0f, t1 = tmax;
    float oo[3] = { o.x, o.y, o.z }, ii[3] = { inv.x, inv.y, inv.z };
    for (int a = 0; a < 3; a++) {
        float ta = (n->lo[a] - oo[a]) * ii[a];
        float tb = (n->hi[a] - oo[a]) * ii[a];
        if (ta > tb) {
            float t = ta;
            ta = tb;
            tb = t;
        }
        if (ta > t0)
            t0 = ta;
        if (tb < t1)
            t1 = tb;
        if (t0 > t1 * 1.0000002f + 1e-6f)
            return 0;
    }
    return 1;
}

static int isect(V3 org, V3 dir, int t, float *lam)
{
    const float eps = 1.1920928955078125e-07f;
    V3 v0 = hv[ht[t * 3]], v1 = hv[ht[t * 3 + 1]], v2 = hv[ht[t * 3 + 2]];
    V3 e1 = sub(v1, v0), e2 = sub(v2, v0);
    V3 p = cross(dir, e2);
    float a = dot(e1, p), f, u, v;
    V3 s, q;
    if (a == 0.0f)
        return 0;
    f = 1.0f / a;
    s = sub(org, v0);
    u = f * dot(s, p);
    if (u < -eps || u > 1.0f + eps)
        return 0;
    q = cross(s, e1);
    v = f * dot(dir, q);
    if (v < -eps || u + v > 1.0f + eps)
        return 0;
    *lam = f * dot(e2, q);
    if (*lam < 0.0f)
        return 0;
    return 1;
}

static int raycast(V3 org, V3 dir, float *best)
{
    int stack[256], sp = 0, hit = -1;
    V3 inv = v3(dir.x != 0.0f ? 1.0f / dir.x : 1e30f, dir.y != 0.0f ? 1.0f / dir.y : 1e30f,
                dir.z != 0.0f ? 1.0f / dir.z : 1e30f);
    float tie = 0.0f;
    *best = 3.4e38f;
    if (nnodes == 0)
        return -1;
    stack[sp++] = 0;
    while (sp) {
        const Node *n = &nodes[stack[--sp]];
        if (!box_hit(n, org, inv, *best * (1.0f + tie) + tie))
            continue;
        if (n->left < 0) {
            for (int i = n->first; i < n->first + n->count; i++) {
                float lam;
                int t = tri_idx[i];
                if (!isect(org, dir, t, &lam))
                    continue;
                if (hit < 0) {
                    *best = lam;
                    hit = t;
                    continue;
                }
                float d = lam - *best, tol = tie * (1.0f + (*best > 0 ? *best : 0));
                if (d < -tol || (d <= tol && t > hit)) {
                    if (lam < *best || d < -tol)
                        *best = lam;
                    hit = t;
                } else if (d < 0) {
                    *best = lam;
                }
            }
        } else {
            stack[sp++] = n->left;
            stack[sp++] = n->right;
        }
    }
    return hit;
}

static int wrap_i(int i, int n, int mode)
{
    if (mode == 1) {
        i %= n;
        if (i < 0)
            i += n;
        return i;
    }
    if (mode == 2) {
        int p = 2 * n;
        i %= p;
        if (i < 0)
            i += p;
        return i >= n ? p - 1 - i : i;
    }
    return i < 0 ? 0 : i >= n ? n - 1 : i;
}

static void tex_sample(const Mat *m, float u, float v, float out[4])
{
    const Tex *tx = &texs[m->tex];
    int w = tx->w, h = tx->h, mode_s = m->wrap_s, mode_t = m->wrap_t;
    float x, y, fx, fy;
    int x0, y0;
    if (mode_s != mode_t) {
        if (mode_s == 1)
            u = u - floorf(u);
        else if (mode_s == 2) {
            float t = u - 2.0f * floorf(u * 0.5f);
            u = t > 1.0f ? 2.0f - t : t;
        }
        if (mode_t == 1)
            v = v - floorf(v);
        else if (mode_t == 2) {
            float t = v - 2.0f * floorf(v * 0.5f);
            v = t > 1.0f ? 2.0f - t : t;
        }
        mode_s = mode_t = 0;
    }
    x = u * (float)w - 0.5f;
    y = v * (float)h - 0.5f;
    x0 = (int)floorf(x);
    y0 = (int)floorf(y);
    fx = x - (float)x0;
    fy = y - (float)y0;
    int xa = wrap_i(x0, w, mode_s), xb = wrap_i(x0 + 1, w, mode_s);
    int ya = wrap_i(y0, h, mode_t), yb = wrap_i(y0 + 1, h, mode_t);
    const unsigned char *p00 = tx->px + (ya * w + xa) * 4, *p10 = tx->px + (ya * w + xb) * 4;
    const unsigned char *p01 = tx->px + (yb * w + xa) * 4, *p11 = tx->px + (yb * w + xb) * 4;
    float w00 = (1.0f - fx) * (1.0f - fy), w10 = fx * (1.0f - fy), w01 = (1.0f - fx) * fy, w11 = fx * fy;
    float a00 = p00[3] / 255.0f, a10 = p10[3] / 255.0f, a01 = p01[3] / 255.0f, a11 = p11[3] / 255.0f;
    float al = w00 * a00 + w10 * a10 + w01 * a01 + w11 * a11;
    for (int c = 0; c < 3; c++) {
        float s = w00 * (p00[c] / 255.0f) + w10 * (p10[c] / 255.0f) + w01 * (p01[c] / 255.0f) +
                  w11 * (p11[c] / 255.0f);
        out[c] = (float)srgb_to_lin(s);
    }
    out[3] = al;
}

static float facing(int t, float bu, float bv)
{
    int a = ht[t * 3], b = ht[t * 3 + 1], c = ht[t * 3 + 2];
    V3 ng = cross(sub(hv[b], hv[a]), sub(hv[c], hv[a]));
    float bw = 1.0f - bu - bv;
    V3 ns = v3(bu * hnrm[a * 3] + bv * hnrm[b * 3] + bw * hnrm[c * 3],
               bu * hnrm[a * 3 + 1] + bv * hnrm[b * 3 + 1] + bw * hnrm[c * 3 + 1],
               bu * hnrm[a * 3 + 2] + bv * hnrm[b * 3 + 2] + bw * hnrm[c * 3 + 2]);
    return dot(ns, ng) < 0.0f ? 0.9086545705795288f : 0.9862135648727417f;
}

static void shade(int t, float bu, float bv, int alpha_mul, float out[4])
{
    const Mat *m = &mats[hmat[t]];
    int a = ht[t * 3], b = ht[t * 3 + 1], c = ht[t * 3 + 2];
    float bw = 1.0f - bu - bv;
    float col[4];
    for (int k = 0; k < 4; k++)
        col[k] = m->factor[k];
    if (m->tex >= 0) {
        float u = bu * huv[a * 2] + bv * huv[b * 2] + bw * huv[c * 2];
        float v = bu * huv[a * 2 + 1] + bv * huv[b * 2 + 1] + bw * huv[c * 2 + 1];
        float tc[4];
        if (m->has_xf) {
            u = u * m->xf[2] + m->xf[0];
            v = v * m->xf[3] + m->xf[1];
        }
        tex_sample(m, u, v, tc);
        for (int k = 0; k < 4; k++)
            col[k] *= tc[k];
    }
    for (int k = 0; k < 4; k++)
        col[k] *= bu * hcol[a * 4 + k] + bv * hcol[b * 4 + k] + bw * hcol[c * 4 + k];
    if (m->alpha_mode == 0)
        col[3] = 1.0f;
    else if (m->alpha_mode == 1)
        col[3] = col[3] < m->cutoff ? 0.0f : 1.0f;
    if (alpha_mul)
        for (int k = 0; k < 3; k++)
            col[k] *= col[3];
    float f = facing(t, bu, bv);
    for (int k = 0; k < 3; k++)
        col[k] *= f;
    for (int k = 0; k < 4; k++)
        out[k] = col[k];
}

typedef struct {
    int tri;
    float u, v, dudx, dudy, dvdx, dvdy;
} Pix;

static unsigned char *img;
static char *mask;
static Pix *pix;
static float span1[4096], span2[4096];
static int miny1, miny2, maxy1, maxy2;
static const float *minp1, *maxp1, *minp2, *maxp2;
static int cur_tri;
static float cur_d[4];
static int rast_mode;
static int *pmap;
static int cur_val;

static void add_span(const float *v1, const float *v2, int recty)
{
    const float *minv, *maxv;
    float *span;
    float xx1, dx0, xs0;
    int y, my0, my2;
    if (v1[1] < v2[1]) {
        minv = v1;
        maxv = v2;
    } else {
        minv = v2;
        maxv = v1;
    }
    my0 = (int)ceilf(minv[1]);
    my2 = (int)floorf(maxv[1]);
    if (my2 < 0 || my0 >= recty)
        return;
    if (my2 >= recty)
        my2 = recty - 1;
    if (my0 < 0)
        my0 = 0;
    if (my0 > my2)
        return;
    xx1 = maxv[1] - minv[1];
    if (xx1 > 1.1920928955078125e-07f) {
        dx0 = (minv[0] - maxv[0]) / xx1;
        xs0 = dx0 * (minv[1] - (float)my2) + minv[0];
    } else {
        dx0 = 0.0f;
        xs0 = minv[0] < maxv[0] ? minv[0] : maxv[0];
    }
    if (maxp1 == NULL)
        span = span1;
    else if (maxv == minp1 || minv == maxp1)
        span = span1;
    else
        span = span2;
    if (span == span1) {
        if (minp1 == NULL || minp1[1] > minv[1])
            minp1 = minv;
        if (maxp1 == NULL || maxp1[1] < maxv[1])
            maxp1 = maxv;
        if (my0 < miny1)
            miny1 = my0;
        if (my2 > maxy1)
            maxy1 = my2;
    } else {
        if (minp2 == NULL || minp2[1] > minv[1])
            minp2 = minv;
        if (maxp2 == NULL || maxp2[1] < maxv[1])
            maxp2 = maxv;
        if (my0 < miny2)
            miny2 = my0;
        if (my2 > maxy2)
            maxy2 = my2;
    }
    for (y = my2; y >= my0; y--, xs0 += dx0)
        span[y] = xs0;
}

static void store(int x, int y, float u, float v)
{
    if (rast_mode == 0) {
        Pix *p = &pix[y * W + x];
        p->tri = cur_tri;
        p->u = u;
        p->v = v;
        p->dudx = cur_d[0];
        p->dudy = cur_d[1];
        p->dvdx = cur_d[2];
        p->dvdy = cur_d[3];
    } else if (mask[y * W + x]) {
        pmap[y * W + x] = cur_val;
    }
}

static void scanconvert(const float *v1, const float *v2, const float *v3)
{
    float x0, y0, x1, y1, x2, y2, z0, z1, z2;
    float u, v, uxd, uyd, vxd, vyd, uy0, vy0, xx1;
    int x, y, sn1, sn2, my0, my2, rectx = W, recty = H;
    miny1 = miny2 = recty + 1;
    maxy1 = maxy2 = -1;
    minp1 = maxp1 = minp2 = maxp2 = NULL;
    add_span(v1, v2, recty);
    add_span(v2, v3, recty);
    add_span(v3, v1, recty);
    if (minp2 == NULL || maxp2 == NULL)
        return;
    my0 = miny1 > miny2 ? miny1 : miny2;
    my2 = maxy1 < maxy2 ? maxy1 : maxy2;
    if (my2 < my0)
        return;
    x1 = v1[0] - v2[0];
    x2 = v2[0] - v3[0];
    y1 = v1[1] - v2[1];
    y2 = v2[1] - v3[1];
    z1 = 1.0f;
    z2 = 0.0f;
    x0 = y1 * z2 - z1 * y2;
    y0 = z1 * x2 - x1 * z2;
    z0 = x1 * y2 - y1 * x2;
    if (z0 == 0.0f)
        return;
    xx1 = (x0 * v1[0] + y0 * v1[1]) / z0 + 1.0f;
    uxd = (float)(-(double)x0 / (double)z0);
    uyd = (float)(-(double)y0 / (double)z0);
    uy0 = (float)((double)my2 * (double)uyd + (double)xx1);
    z1 = -1.0f;
    z2 = 1.0f;
    x0 = y1 * z2 - z1 * y2;
    y0 = z1 * x2 - x1 * z2;
    xx1 = (x0 * v1[0] + y0 * v1[1]) / z0;
    vxd = (float)(-(double)x0 / (double)z0);
    vyd = (float)(-(double)y0 / (double)z0);
    vy0 = (float)((double)my2 * (double)vyd + (double)xx1);
    for (y = my2; y >= my0; y--) {
        float a = span1[y], b = span2[y];
        sn1 = (int)floorf(a < b ? a : b);
        sn2 = (int)floorf(a > b ? a : b);
        sn1++;
        if (sn2 >= rectx)
            sn2 = rectx - 1;
        if (sn1 < 0)
            sn1 = 0;
        u = (float)((double)sn1 * (double)uxd + (double)uy0);
        v = (float)((double)sn1 * (double)vxd + (double)vy0);
        for (x = sn1; x <= sn2; x++) {
            store(x, y, u, v);
            u += uxd;
            v += vxd;
        }
        uy0 -= uyd;
        vy0 -= vyd;
    }
}

static void uv_xy(const float *uv, float out[2])
{
    out[0] = uv[0] * (float)W - (0.5f + 0.001f);
    out[1] = uv[1] * (float)H - (0.5f + 0.002f);
}

static void filter_extend(unsigned char *ib, char *mk, int passes)
{
    static const float wt[9] = { 1, 2, 1, 2, 0, 2, 1, 2, 1 };
    unsigned char *dst = malloc((size_t)W * H * 4);
    char *dm = malloc((size_t)W * H);
    int more = 1;
    for (int r = 0; more && r < passes; r++) {
        more = 0;
        memcpy(dst, ib, (size_t)W * H * 4);
        memcpy(dm, mk, (size_t)W * H);
        for (int y = 0; y < H; y++)
            for (int x = 0; x < W; x++) {
                int idx = y * W + x;
                if (mk[idx])
                    continue;
                if (!((x > 0 && mk[idx - 1]) || (x < W - 1 && mk[idx + 1]) || (y > 0 && mk[idx - W]) ||
                      (y < H - 1 && mk[idx + W])))
                    continue;
                float acc[4] = { 0, 0, 0, 0 }, ws = 0;
                int k = 0;
                for (int i = -1; i <= 1; i++)
                    for (int j = -1; j <= 1; j++) {
                        if (i != 0 || j != 0) {
                            int xx = x + i, yy = y + j;
                            if (xx >= 0 && xx < W && yy >= 0 && yy < H && mk[yy * W + xx]) {
                                ws += wt[k];
                                for (int c = 0; c < 4; c++)
                                    acc[c] += wt[k] * (float)ib[(yy * W + xx) * 4 + c];
                            }
                        }
                        k++;
                    }
                if (ws != 0) {
                    for (int c = 0; c < 4; c++) {
                        float a = acc[c] / ws;
                        dst[idx * 4 + c] = a > 255 ? 255 : a < 0 ? 0 : (unsigned char)floorf(a + 0.5f);
                    }
                    dm[idx] = 2;
                    more = 1;
                }
            }
        memcpy(ib, dst, (size_t)W * H * 4);
        memcpy(mk, dm, (size_t)W * H);
    }
    free(dst);
    free(dm);
}

static const int dirs[8][2] = { { -1, 0 }, { -1, -1 }, { 0, -1 }, { 1, -1 },
                                { 1, 0 },  { 1, 1 },   { 0, 1 },  { -1, 1 } };
static const int dists[8] = { 2, 3, 2, 3, 2, 3, 2, 3 };
#define UNSET 0xFFFFFFFFu
#define ISDJ(p) ((p) & 0x80000000u)
#define PACKDJ(d, r) (0x80000000u + ((unsigned)(d) << 4) + (unsigned)(r))
#define DJDIST(p) ((((p) ^ 0x80000000u)) >> 4)
#define DJDIR(p) ((p) & 0xFu)

static unsigned *map;
static int *loop_adj, *loop_poly;

static unsigned getp(int x, int y)
{
    if (x < 0 || y < 0 || x >= W || y >= H)
        return UNSET;
    return map[y * W + x];
}

typedef struct {
    int d, x, y;
} HeapItem;

static HeapItem *heap;
static int hn;

static int hless(HeapItem a, HeapItem b)
{
    return a.d > b.d;
}

static void heap_push(void)
{
    int c = hn - 1;
    HeapItem t = heap[c];
    while (c > 0) {
        int p = (c - 1) / 2;
        if (hless(heap[p], t)) {
            heap[c] = heap[p];
            c = p;
        } else
            break;
    }
    heap[c] = t;
}

static void heap_pop(void)
{
    HeapItem top = heap[0];
    int len = hn, c = 0;
    HeapItem last;
    if (len <= 1)
        return;
    last = heap[len - 1];
    for (;;) {
        int l = 2 * c + 1;
        if (l >= len - 1)
            break;
        if (l + 1 < len - 1 && hless(heap[l], heap[l + 1]))
            l++;
        if (hless(last, heap[l])) {
            heap[c] = heap[l];
            c = l;
        } else
            break;
    }
    heap[c] = last;
    heap[len - 1] = top;
}

static void uvxy_loop(int l, float out[2])
{
    uv_xy(&ploopuv[l * 2], out);
}

static int lookup_pixel(float x, float y, int src, float *rx, float *ry, int *rother, float *rdist)
{
    int found = -1;
    float fdist = -1, ft = 0;
    *rx = *ry = 0;
    for (int i = 0; i < pcount[src]; i++) {
        int l1 = pstart[src] + i, l2 = l1 + 1;
        float e1[2], e2[2], ab[2], ap[2], dotv, al, t;
        if (l2 >= pstart[src] + pcount[src])
            l2 = pstart[src];
        uvxy_loop(l1, e1);
        uvxy_loop(l2, e2);
        ab[0] = e2[0] - e1[0];
        ab[1] = e2[1] - e1[1];
        ap[0] = x - e1[0];
        ap[1] = y - e1[1];
        dotv = ab[0] * ap[0] + ab[1] * ap[1];
        al = ab[0] * ab[0] + ab[1] * ab[1];
        t = dotv / al;
        if (t >= 0.0f && t <= 1.0f) {
            float rp[2], rv[2], rl, cr;
            rp[0] = e1[0] + t * ab[0];
            rp[1] = e1[1] + t * ab[1];
            rv[0] = rp[0] - x;
            rv[1] = rp[1] - y;
            rl = sqrtf(rv[0] * rv[0] + rv[1] * rv[1]);
            cr = ab[0] * rv[1] - ab[1] * rv[0];
            if (cr > 0.0f && (fdist < 0 || rl < fdist)) {
                fdist = rl;
                ft = t;
                found = l1;
            }
        }
    }
    if (found < 0)
        return 0;
    if (rdist)
        *rdist = fdist;
    int oe = loop_adj[found], dp, oe2;
    float o1[2], o2[2], oab[2], orp[2], perp[2], len;
    if (oe < 0)
        return 0;
    dp = loop_poly[oe];
    *rother = dp;
    oe2 = oe + 1;
    if (oe2 >= pstart[dp] + pcount[dp])
        oe2 = pstart[dp];
    uvxy_loop(oe, o1);
    uvxy_loop(oe2, o2);
    oab[0] = o1[0] - o2[0];
    oab[1] = o1[1] - o2[1];
    orp[0] = o2[0] + ft * oab[0];
    orp[1] = o2[1] + ft * oab[1];
    perp[0] = oab[1];
    perp[1] = -oab[0];
    len = sqrtf(perp[0] * perp[0] + perp[1] * perp[1]);
    *rx = orp[0] + (fdist / len) * perp[0];
    *ry = orp[1] + (fdist / len) * perp[1];
    return 1;
}

static int lookup_nb(float x, float y, int cx, int cy, unsigned *start, float *rx, float *ry, int *rother)
{
    float mind = -1;
    if (lookup_pixel(x, y, (int)*start, rx, ry, rother, NULL))
        return 1;
    for (int dy = -1; dy <= 1; dy++)
        for (int dx = -1; dx <= 1; dx++) {
            unsigned op;
            float d, ex, ey;
            int fp;
            if (dx == 0 && dy == 0)
                continue;
            op = getp(cx + dx, cy + dy);
            if (op == UNSET || ISDJ(op) || op == *start)
                continue;
            if (lookup_pixel(x, y, (int)op, &ex, &ey, &fp, &d)) {
                if (mind < 0 || d < mind) {
                    mind = d;
                    *rother = fp;
                    *rx = ex;
                    *ry = ey;
                    *start = op;
                }
            }
        }
    return mind >= 0;
}

static void bilinear_byte(float u, float v, unsigned char out[4])
{
    float uf = floorf(u), vf = floorf(v), a, b;
    int x1 = (int)uf, x2 = x1 + 1, y1 = (int)vf, y2 = y1 + 1;
    static const unsigned char empty[4] = { 0, 0, 0, 0 };
    const unsigned char *r1, *r2, *r3, *r4;
    if (x2 < 0 || x1 >= W || y2 < 0 || y1 >= H) {
        memset(out, 0, 4);
        return;
    }
    r1 = (x1 < 0 || y1 < 0) ? empty : img + (y1 * W + x1) * 4;
    r2 = (x1 < 0 || y2 > H - 1) ? empty : img + (y2 * W + x1) * 4;
    r3 = (x2 > W - 1 || y1 < 0) ? empty : img + (y1 * W + x2) * 4;
    r4 = (x2 > W - 1 || y2 > H - 1) ? empty : img + (y2 * W + x2) * 4;
    a = u - uf;
    b = v - vf;
    float ab = a * b, mab = (1.0f - a) * b, amb = a * (1.0f - b), mamb = (1.0f - a) * (1.0f - b);
    for (int c = 0; c < 4; c++)
        out[c] = (unsigned char)(mamb * r1[c] + amb * r3[c] + mab * r2[c] + ab * r4[c] + 0.5f);
}

static void margin_adjacent(void)
{
    char *tmpm;
    map = malloc(sizeof(unsigned) * W * H);
    for (int i = 0; i < W * H; i++)
        map[i] = UNSET;
    rast_mode = 1;
    pmap = (int *)map;
    for (int t = 0; t < nlt; t++) {
        float a[2], b[2], c[2];
        uv_xy(&luv[t * 6], a);
        uv_xy(&luv[t * 6 + 2], b);
        uv_xy(&luv[t * 6 + 4], c);
        cur_val = lpoly[t];
        scanconvert(a, b, c);
    }
    tmpm = malloc((size_t)W * H);
    memcpy(tmpm, mask, (size_t)W * H);
    filter_extend(img, tmpm, 2);
    free(tmpm);
    heap = malloc(sizeof(HeapItem) * W * H * 9);
    hn = 0;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) {
            if (getp(x, y) != UNSET)
                continue;
            for (int i = 0; i < 8; i++) {
                int xx = x - dirs[i][0], yy = y - dirs[i][1];
                if (xx >= 0 && xx < W && yy >= 0 && yy < W && !ISDJ(getp(xx, yy))) {
                    map[y * W + x] = PACKDJ(dists[i], i);
                    heap[hn].d = dists[i];
                    heap[hn].x = x;
                    heap[hn].y = y;
                    hn++;
                    break;
                }
            }
        }
    while (hn) {
        HeapItem p;
        heap_pop();
        p = heap[--hn];
        if (p.d < 2 * (MARGIN + 1)) {
            for (int i = 0; i < 8; i++) {
                int x = p.x + dirs[i][0], y = p.y + dirs[i][1];
                if (x >= 0 && x < W && y >= 0 && y < H) {
                    unsigned dp = getp(x, y);
                    if (ISDJ(dp) && DJDIST(dp) > (unsigned)(p.d + dists[i])) {
                        map[y * W + x] = PACKDJ(p.d + dists[i], i);
                        heap[hn].d = p.d + dists[i];
                        heap[hn].x = x;
                        heap[hn].y = y;
                        hn++;
                        heap_push();
                    }
                }
            }
        }
    }
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) {
            unsigned dp = getp(x, y);
            if (ISDJ(dp) && dp != UNSET) {
                int dist = (int)DJDIST(dp), dir = (int)DJDIR(dp), xx = x, yy = y;
                unsigned poly;
                float dx, dy;
                int other, found = 0;
                while (dist > 0) {
                    xx -= dirs[dir][0];
                    yy -= dirs[dir][1];
                    dp = getp(xx, yy);
                    dist -= dists[dir];
                    dir = (int)DJDIR(dp);
                }
                poly = getp(xx, yy);
                if (lookup_nb((float)x, (float)y, xx, yy, &poly, &dx, &dy, &other)) {
                    for (int i = 0; i < 3; i++) {
                        int nx = (int)lroundf(dx), ny = (int)lroundf(dy);
                        unsigned pm = getp(nx, ny);
                        if ((unsigned)other == pm) {
                            found = 1;
                            break;
                        }
                        if (!lookup_pixel((float)nx, (float)ny, other, &dx, &dy, &other, NULL)) {
                            found = 0;
                            break;
                        }
                    }
                    if (found) {
                        bilinear_byte(dx, dy, img + (y * W + x) * 4);
                        mask[y * W + x] = 1;
                    }
                }
            } else {
                mask[y * W + x] = 1;
            }
        }
    filter_extend(img, mask, MARGIN);
    free(heap);
    free(map);
}

static V3 closest_on_tri(V3 p, V3 a, V3 b, V3 c)
{
    V3 ab = sub(b, a), ac = sub(c, a), ap = sub(p, a), bp, cp;
    double d1 = dot(ab, ap), d2 = dot(ac, ap), d3, d4, d5, d6, va, vb, vc, v, w, den;
    if (d1 <= 0.0 && d2 <= 0.0)
        return a;
    bp = sub(p, b);
    d3 = dot(ab, bp);
    d4 = dot(ac, bp);
    if (d3 >= 0.0 && d4 <= d3)
        return b;
    vc = d1 * d4 - d3 * d2;
    if (vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0) {
        v = d1 / (d1 - d3);
        return add(a, mulf(ab, (float)v));
    }
    cp = sub(p, c);
    d5 = dot(ab, cp);
    d6 = dot(ac, cp);
    if (d6 >= 0.0 && d5 <= d6)
        return c;
    vb = d5 * d2 - d1 * d6;
    if (vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0) {
        w = d2 / (d2 - d6);
        return add(a, mulf(ac, (float)w));
    }
    va = d3 * d6 - d5 * d4;
    if (va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0) {
        w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, mulf(sub(c, b), (float)w));
    }
    den = 1.0 / (va + vb + vc);
    v = vb * den;
    w = vc * den;
    return add(a, add(mulf(ab, (float)v), mulf(ac, (float)w)));
}

static int closest_tri(V3 p, V3 *best, double *bd)
{
    int bt = -1;
    for (int h = 0; h < nht; h++) {
        V3 q = closest_on_tri(p, hv[ht[h * 3]], hv[ht[h * 3 + 1]], hv[ht[h * 3 + 2]]);
        V3 d = sub(q, p);
        double dd = (double)d.x * d.x + (double)d.y * d.y + (double)d.z * d.z;
        if (dd < *bd) {
            *bd = dd;
            bt = h;
            *best = q;
        }
    }
    return bt;
}

static int nearest_tri(V3 p, int want_alpha, V3 *loc, float *dist)
{
    int best = -1;
    double bd = 1e300;
    for (int t = 0; t < nht; t++) {
        int isa = mats[hmat[t]].alpha_mode != 0;
        V3 q, d;
        double dd;
        if (isa != want_alpha)
            continue;
        if (hpart && cur_part >= 0 && hpart[t] != cur_part)
            continue;
        q = closest_on_tri(p, hv[ht[t * 3]], hv[ht[t * 3 + 1]], hv[ht[t * 3 + 2]]);
        d = sub(q, p);
        dd = (double)d.x * d.x + (double)d.y * d.y + (double)d.z * d.z;
        if (dd < bd) {
            bd = dd;
            best = t;
            *loc = q;
        }
    }
    *dist = (float)sqrt(bd);
    return best;
}

static void bary_of(int t, V3 p, float *bu, float *bv)
{
    V3 a = hv[ht[t * 3]], b = hv[ht[t * 3 + 1]], c = hv[ht[t * 3 + 2]];
    V3 v0 = sub(b, a), v1 = sub(c, a), v2 = sub(p, a);
    double d00 = dot(v0, v0), d01 = dot(v0, v1), d11 = dot(v1, v1), d20 = dot(v2, v0), d21 = dot(v2, v1);
    double den = d00 * d11 - d01 * d01, v, w;
    if (den == 0.0) {
        *bu = 1.0f;
        *bv = 0.0f;
        return;
    }
    v = (d11 * d20 - d01 * d21) / den;
    w = (d00 * d21 - d01 * d20) / den;
    *bu = (float)(1.0 - v - w);
    *bv = (float)v;
}

static void hit_colour(int t, V3 loc, float out[4])
{
    float bu, bv;
    const Mat *m = &mats[hmat[t]];
    bary_of(t, loc, &bu, &bv);
    shade(t, bu, bv, 0, out);
    float f = facing(t, bu, bv);
    for (int k = 0; k < 3; k++)
        out[k] /= f;
    if (m->alpha_mode == 0)
        out[3] = 1.0f;
    else
        out[3] = out[3] >= (m->alpha_mode == 1 ? 1.0f : 0.05f) ? 1.0f : 0.0f;
}

static int raycast_sel(V3 org, V3 dir, float maxd, int want_alpha, float *best)
{
    int stack[256], sp = 0, hit = -1;
    V3 inv = v3(dir.x != 0.0f ? 1.0f / dir.x : 1e30f, dir.y != 0.0f ? 1.0f / dir.y : 1e30f,
                dir.z != 0.0f ? 1.0f / dir.z : 1e30f);
    *best = maxd;
    if (nnodes == 0)
        return -1;
    stack[sp++] = 0;
    while (sp) {
        const Node *n = &nodes[stack[--sp]];
        if (!box_hit(n, org, inv, *best))
            continue;
        if (n->left < 0) {
            for (int i = n->first; i < n->first + n->count; i++) {
                float lam;
                int t = tri_idx[i];
                if ((mats[hmat[t]].alpha_mode != 0) != want_alpha)
                    continue;
                if (hpart && cur_part >= 0 && hpart[t] != cur_part)
                    continue;
                if (isect(org, dir, t, &lam) && lam <= *best) {
                    *best = lam;
                    hit = t;
                }
            }
        } else {
            stack[sp++] = n->left;
            stack[sp++] = n->right;
        }
    }
    return hit;
}

static float DIAG;

static int colour_at(V3 p, V3 n, float reach, float out[4], float *rdist)
{
    float col[2][4], dd[2];
    int has[2] = { 0, 0 }, ray[2] = { 0, 0 };
    for (int a = 0; a < 2; a++) {
        float lam;
        V3 start = add(p, mulf(n, reach + DIAG * 0.01f));
        V3 dir = mulf(n, -1.0f);
        int t = raycast_sel(start, dir, 2.0f * (reach + DIAG * 0.01f) + DIAG * 0.04f, a, &lam);
        if (t >= 0) {
            V3 loc = add(start, mulf(dir, lam));
            V3 d = sub(loc, p);
            hit_colour(t, loc, col[a]);
            dd[a] = sqrtf(dot(d, d));
            has[a] = 1;
            ray[a] = 1;
        } else {
            V3 loc;
            float di;
            int t2 = nearest_tri(p, a, &loc, &di);
            if (t2 >= 0) {
                hit_colour(t2, loc, col[a]);
                dd[a] = di;
                has[a] = 1;
            }
        }
    }
    if (has[1] && col[1][3] > 0.0f && (!has[0] || dd[1] <= dd[0] + DIAG * 0.01f)) {
        memcpy(out, col[1], 16);
        *rdist = dd[1];
        return ray[1];
    }
    if (has[0]) {
        memcpy(out, col[0], 16);
        *rdist = dd[0];
        return ray[0];
    }
    if (has[1]) {
        memcpy(out, col[1], 16);
        *rdist = dd[1];
        return ray[1];
    }
    out[0] = out[1] = out[2] = (float)srgb_to_lin(0.5);
    out[3] = 1.0f;
    *rdist = 1e9f;
    return 0;
}

static int closest_bary2(float px, float py, const float *a, const float *b, const float *c, double bc[3],
                         double *dist)
{
    double v0x = b[0] - a[0], v0y = b[1] - a[1], v1x = c[0] - a[0], v1y = c[1] - a[1], v2x = px - a[0],
           v2y = py - a[1];
    double d00 = v0x * v0x + v0y * v0y, d01 = v0x * v1x + v0y * v1y, d11 = v1x * v1x + v1y * v1y;
    double d20 = v2x * v0x + v2y * v0y, d21 = v2x * v1x + v2y * v1y, den = d00 * d11 - d01 * d01, v, w, u;
    const float *S[3] = { a, b, c }, *T[3] = { b, c, a };
    int IA[3] = { 0, 1, 2 }, IB[3] = { 1, 2, 0 };
    double bestd = -1;
    if (den < 0 ? -den < 1e-14 : den < 1e-14) {
        *dist = 1e9;
        return 0;
    }
    v = (d11 * d20 - d01 * d21) / den;
    w = (d00 * d21 - d01 * d20) / den;
    u = 1 - v - w;
    if (u >= 0 && v >= 0 && w >= 0) {
        bc[0] = u;
        bc[1] = v;
        bc[2] = w;
        *dist = 0.0;
        return 1;
    }
    for (int e = 0; e < 3; e++) {
        double ex = T[e][0] - S[e][0], ey = T[e][1] - S[e][1], l = ex * ex + ey * ey, k, qx, qy, d;
        if (l < 1e-14)
            l = 1e-14;
        k = ((px - S[e][0]) * ex + (py - S[e][1]) * ey) / l;
        if (k < 0)
            k = 0;
        if (k > 1)
            k = 1;
        qx = S[e][0] + ex * k;
        qy = S[e][1] + ey * k;
        d = sqrt((px - qx) * (px - qx) + (py - qy) * (py - qy));
        if (bestd < 0 || d < bestd) {
            bestd = d;
            bc[0] = bc[1] = bc[2] = 0;
            bc[IA[e]] = 1 - k;
            bc[IB[e]] = k;
        }
    }
    *dist = bestd;
    return 1;
}

static void run_parts(void)
{
    int R = W, cut = (FLAGS & FLAG_CUT) != 0;
    float *px = calloc((size_t)R * R * 4, sizeof(float));
    double *qual = malloc(sizeof(double) * R * R);
    char *filled = calloc((size_t)R * R, 1);
    float lo[3] = { 1e30f, 1e30f, 1e30f }, hi[3] = { -1e30f, -1e30f, -1e30f };
    for (int t = 0; t < nht; t++)
        for (int k = 0; k < 3; k++) {
            V3 p = hv[ht[t * 3 + k]];
            float c[3] = { p.x, p.y, p.z };
            for (int a = 0; a < 3; a++) {
                if (c[a] < lo[a])
                    lo[a] = c[a];
                if (c[a] > hi[a])
                    hi[a] = c[a];
            }
        }
    DIAG = sqrtf((hi[0] - lo[0]) * (hi[0] - lo[0]) + (hi[1] - lo[1]) * (hi[1] - lo[1]) +
                 (hi[2] - lo[2]) * (hi[2] - lo[2]));
    for (int i = 0; i < R * R; i++)
        qual[i] = 1e9;
    for (int t = 0; t < nlt; t++) {
        float uv[3][2];
        cur_part = -1;
        if (hpart) {
            int nparts = 0;
            double *acc;
            for (int i = 0; i < nht; i++)
                if (hpart[i] + 1 > nparts)
                    nparts = hpart[i] + 1;
            acc = malloc(sizeof(double) * (nparts * 4 + 1));
            for (int i = 0; i < nparts * 4; i++)
                acc[i] = 1e300;
            for (int k = 0; k < 4; k++) {
                V3 q = k < 3 ? lv[lt[t * 3 + k]]
                             : mulf(add(add(lv[lt[t * 3]], lv[lt[t * 3 + 1]]), lv[lt[t * 3 + 2]]),
                                    1.0f / 3.0f);
                for (int h = 0; h < nht; h++) {
                    V3 c = closest_on_tri(q, hv[ht[h * 3]], hv[ht[h * 3 + 1]], hv[ht[h * 3 + 2]]);
                    V3 d = sub(c, q);
                    double dd = sqrt((double)d.x * d.x + (double)d.y * d.y + (double)d.z * d.z);
                    if (dd < acc[hpart[h] * 4 + k])
                        acc[hpart[h] * 4 + k] = dd;
                }
            }
            double best = 1e300;
            int *hits = calloc((size_t)nparts + 1, sizeof(int));
            for (int k = 0; k < 3; k++) {
                V3 q = lv[lt[t * 3 + k]];
                for (int h = 0; h < nht; h++)
                    for (int c = 0; c < 3; c++) {
                        V3 d = sub(hv[ht[h * 3 + c]], q);
                        if (dot(d, d) < 1e-4f)
                            hits[hpart[h]] |= 1 << k;
                    }
            }
            for (int pi = 0; pi < nparts; pi++) {
                int nb = (hits[pi] & 1) + ((hits[pi] >> 1) & 1) + ((hits[pi] >> 2) & 1);
                double sc = acc[pi * 4] + acc[pi * 4 + 1] + acc[pi * 4 + 2] + acc[pi * 4 + 3] - nb * 1e9;
                if (sc < best) {
                    best = sc;
                    cur_part = pi;
                }
            }
            free(hits);
            free(acc);
        }
        V3 P[3], nrm;
        int x0, x1, y0, y1;
        float mnx = 1e30f, mxx = -1e30f, mny = 1e30f, mxy = -1e30f;
        for (int k = 0; k < 3; k++) {
            uv[k][0] = luv[t * 6 + k * 2] * R;
            uv[k][1] = luv[t * 6 + k * 2 + 1] * R;
            P[k] = lv[lt[t * 3 + k]];
            if (uv[k][0] < mnx)
                mnx = uv[k][0];
            if (uv[k][0] > mxx)
                mxx = uv[k][0];
            if (uv[k][1] < mny)
                mny = uv[k][1];
            if (uv[k][1] > mxy)
                mxy = uv[k][1];
        }
        nrm = norm(cross(sub(P[1], P[0]), sub(P[2], P[0])));
        x0 = (int)floorf(mnx - 1);
        if (x0 < 0)
            x0 = 0;
        x1 = (int)ceilf(mxx + 1);
        if (x1 > R - 1)
            x1 = R - 1;
        y0 = (int)floorf(mny - 1);
        if (y0 < 0)
            y0 = 0;
        y1 = (int)ceilf(mxy + 1);
        if (y1 > R - 1)
            y1 = R - 1;
        for (int y = y0; y <= y1; y++)
            for (int x = x0; x <= x1; x++) {
                double bc[3], d;
                float col[4], rd;
                V3 p;
                int ray;
                if (!closest_bary2(x + 0.5f, y + 0.5f, uv[0], uv[1], uv[2], bc, &d))
                    continue;
                if (d > 0.8 || d >= qual[y * R + x])
                    continue;
                p = add(add(mulf(P[0], (float)bc[0]), mulf(P[1], (float)bc[1])), mulf(P[2], (float)bc[2]));
                ray = colour_at(p, nrm, (FLAGS & FLAG_FAR) ? DIAG : 0.0f, col, &rd);
                if (cut && !ray)
                    col[3] = 0.0f;
                if (!cut && col[3] < 0.5f)
                    continue;
                memcpy(px + (y * R + x) * 4, col, 16);
                qual[y * R + x] = d;
                filled[y * R + x] = 1;
            }
    }
    float *acc = malloc(sizeof(float) * R * R * 3);
    int *cnt = malloc(sizeof(int) * R * R);
    for (int it = 0; it < R; it++) {
        int all = 1;
        for (int i = 0; i < R * R; i++)
            if (!filled[i]) {
                all = 0;
                break;
            }
        if (all)
            break;
        memset(acc, 0, sizeof(float) * R * R * 3);
        memset(cnt, 0, sizeof(int) * R * R);
        for (int y = 0; y < R; y++)
            for (int x = 0; x < R; x++)
                for (int dy = -1; dy <= 1; dy++)
                    for (int dx = -1; dx <= 1; dx++) {
                        int sy = (y - dy + R) % R, sx = (x - dx + R) % R;
                        if (filled[sy * R + sx]) {
                            for (int c = 0; c < 3; c++)
                                acc[(y * R + x) * 3 + c] += px[(sy * R + sx) * 4 + c];
                            cnt[y * R + x]++;
                        }
                    }
        for (int i = 0; i < R * R; i++)
            if (!filled[i] && cnt[i] > 0) {
                for (int c = 0; c < 3; c++)
                    px[i * 4 + c] = acc[i * 3 + c] / cnt[i];
            }
        for (int i = 0; i < R * R; i++)
            if (!filled[i] && cnt[i] > 0)
                filled[i] = 1;
    }
    free(acc);
    free(cnt);
    if (!cut)
        for (int i = 0; i < R * R; i++)
            px[i * 4 + 3] = 1.0f;
    int h = R / 2;
    unsigned char *res = malloc((size_t)h * h * 4);
    for (int j = 0; j < h; j++)
        for (int i = 0; i < h; i++) {
            double as = 0, cs[3] = { 0, 0, 0 }, cm[3] = { 0, 0, 0 }, col[4];
            for (int dy = 0; dy < 2; dy++)
                for (int dx = 0; dx < 2; dx++) {
                    const float *q = px + ((j * 2 + dy) * R + i * 2 + dx) * 4;
                    as += q[3];
                    for (int c = 0; c < 3; c++) {
                        cs[c] += q[c] * q[3];
                        cm[c] += q[c];
                    }
                }
            for (int c = 0; c < 3; c++)
                col[c] = as > 1e-6 ? cs[c] / (as > 1e-6 ? as : 1e-6) : cm[c] / 4;
            col[3] = as / 4;
            if (cut)
                col[3] = col[3] >= 0.5 ? 1.0 : 0.0;
            for (int c = 0; c < 4; c++) {
                double v = c < 3 ? lin_to_srgb(col[c] < 0 ? 0 : col[c] > 1 ? 1 : col[c]) : col[3];
                float fv = (float)v;
                res[(j * h + i) * 4 + c] = fv <= 0.0f                  ? 0
                                           : fv > 1.0f - 0.5f / 255.0f ? 255
                                                                       : (unsigned char)(255.0f * fv + 0.5f);
            }
        }
    write_file(NULL, res, (size_t)h * h * 4);
    free(res);
    free(px);
    free(qual);
    free(filled);
}

static double tclip(double v, double lo, double hi)
{
    return v < lo ? lo : v > hi ? hi : v;
}

static double tcomb(double a, double b, double c, double d, int op, int bias, int scale, int clamp)
{
    double v;
    a = tclip(a, 0.0, 1.0);
    b = tclip(b, 0.0, 1.0);
    c = tclip(c, 0.0, 1.0);
    v = (1 - c) * a + c * b;
    v = op == 1 ? d - v : d + v;
    v = v + (bias == 1 ? 0.5 : bias == 2 ? -0.5 : 0.0);
    v = v * (scale == 1 ? 2.0 : scale == 2 ? 4.0 : scale == 3 ? 0.5 : 1.0);
    return clamp ? tclip(v, 0.0, 1.0) : tclip(v, -4.0, 4.0);
}

#ifndef TEXTURES_BAKE_CORE_LIBRARY
static void run_tev(const char *in, const char *out)
{
    FILE *g;
    int nm;
    load_input(in);
    g = fopen(out, "wb");
    nm = rd_i();
    for (int mi = 0; mi < nm; mi++) {
        int ph = rd_i(), pw = rd_i(), ns = rd_i(), nt = rd_i(), npx = ph * pw;
        double regs[4][4], ras0[4];
        int tw[8], th[8];
        const unsigned char *tp[8];
        double *prev, *rg[3], *newc, *newa;
        for (int r = 0; r < 4; r++)
            for (int c = 0; c < 4; c++)
                regs[r][c] = rd_f64();
        for (int c = 0; c < 4; c++)
            ras0[c] = rd_f64();
        for (int k = 0; k < 8; k++) {
            tw[k] = 0;
            th[k] = 0;
            tp[k] = NULL;
        }
        for (int k = 0; k < nt; k++) {
            int slot = rd_i();
            tw[slot] = rd_i();
            th[slot] = rd_i();
            tp[slot] = buf + bpos;
            bpos += (long)tw[slot] * th[slot] * 4;
        }
        prev = malloc(sizeof(double) * npx * 4);
        for (int r = 0; r < 3; r++)
            rg[r] = malloc(sizeof(double) * npx * 4);
        newc = malloc(sizeof(double) * npx * 3);
        newa = malloc(sizeof(double) * npx);
        for (int p = 0; p < npx; p++)
            for (int c = 0; c < 4; c++) {
                prev[p * 4 + c] = regs[3][c];
                for (int r = 0; r < 3; r++)
                    rg[r][p * 4 + c] = regs[r][c];
            }
        for (int s = 0; s < ns; s++) {
            int slot = rd_i(), chan = rd_i(), rsw[4], tsw[4], ci[4], ai[4], cop, aop, cb, cs, cc, ab, as, ac,
                creg, areg;
            double kc[4], ka[4], ras[4], *dstc, *dsta;
            for (int c = 0; c < 4; c++)
                rsw[c] = rd_i();
            for (int c = 0; c < 4; c++)
                tsw[c] = rd_i();
            for (int c = 0; c < 4; c++)
                kc[c] = rd_f64();
            for (int c = 0; c < 4; c++)
                ka[c] = rd_f64();
            for (int c = 0; c < 4; c++)
                ci[c] = rd_i();
            for (int c = 0; c < 4; c++)
                ai[c] = rd_i();
            cop = rd_i();
            aop = rd_i();
            cb = rd_i();
            cs = rd_i();
            cc = rd_i();
            ab = rd_i();
            as = rd_i();
            ac = rd_i();
            creg = rd_i();
            areg = rd_i();
            for (int c = 0; c < 4; c++)
                ras[c] = (chan == 0 || chan == 2 || chan == 4) ? ras0[rsw[c]] : 0.0;
            dstc = creg == 0 ? prev : rg[creg - 1];
            dsta = areg == 0 ? prev : rg[areg - 1];
            for (int y = 0; y < ph; y++)
                for (int x = 0; x < pw; x++) {
                    int p = y * pw + x;
                    double t0[4] = { 1.0, 1.0, 1.0, 1.0 }, t[4], *pv = prev + p * 4, *r0 = rg[0] + p * 4,
                           *r1 = rg[1] + p * 4, *r2 = rg[2] + p * 4;
                    double arg[4];
                    if (slot >= 0 && tp[slot]) {
                        int sy = y * th[slot] / ph, sx = x * tw[slot] / pw;
                        const unsigned char *q = tp[slot] + (sy * tw[slot] + sx) * 4;
                        for (int c = 0; c < 4; c++)
                            t0[c] = q[c] / 255.0;
                    }
                    for (int c = 0; c < 4; c++)
                        t[c] = t0[tsw[c]];
                    for (int c = 0; c < 3; c++) {
                        for (int k = 0; k < 4; k++) {
                            int xx = ci[k];
                            arg[k] = xx == 0    ? pv[c]
                                     : xx == 1  ? pv[3]
                                     : xx == 2  ? r0[c]
                                     : xx == 3  ? r0[3]
                                     : xx == 4  ? r1[c]
                                     : xx == 5  ? r1[3]
                                     : xx == 6  ? r2[c]
                                     : xx == 7  ? r2[3]
                                     : xx == 8  ? t[c]
                                     : xx == 9  ? t[3]
                                     : xx == 10 ? ras[c]
                                     : xx == 11 ? ras[3]
                                     : xx == 12 ? 1.0
                                     : xx == 13 ? 0.5
                                     : xx == 14 ? kc[c]
                                                : 0.0;
                        }
                        newc[p * 3 + c] = tcomb(arg[0], arg[1], arg[2], arg[3], cop, cb, cs, cc);
                    }
                    for (int k = 0; k < 4; k++) {
                        int xx = ai[k];
                        arg[k] = xx == 0   ? pv[3]
                                 : xx == 1 ? r0[3]
                                 : xx == 2 ? r1[3]
                                 : xx == 3 ? r2[3]
                                 : xx == 4 ? t[3]
                                 : xx == 5 ? ras[3]
                                 : xx == 6 ? ka[3]
                                           : 0.0;
                    }
                    newa[p] = tcomb(arg[0], arg[1], arg[2], arg[3], aop, ab, as, ac);
                }
            for (int p = 0; p < npx; p++)
                for (int c = 0; c < 3; c++)
                    dstc[p * 4 + c] = newc[p * 3 + c];
            for (int p = 0; p < npx; p++)
                dsta[p * 4 + 3] = newa[p];
        }
        unsigned char *o = malloc((size_t)npx * 4);
        for (int p = 0; p < npx * 4; p++)
            o[p] = (unsigned char)(int)rint(tclip(prev[p], 0.0, 1.0) * 255.0);
        fwrite(o, 1, (size_t)npx * 4, g);
        free(o);
        free(prev);
        free(rg[0]);
        free(rg[1]);
        free(rg[2]);
        free(newc);
        free(newa);
    }
    fclose(g);
    free(owned_buf);
    owned_buf = NULL;
}
#endif

static void uv_range(double a, double b, double c, double *lo, double *hi)
{
    *lo = a;
    *hi = a;
    if (b < *lo)
        *lo = b;
    if (c < *lo)
        *lo = c;
    if (b > *hi)
        *hi = b;
    if (c > *hi)
        *hi = c;
}

static void run_reproj(void)
{
    int Wo = W, Ho = MARGIN;
    const Tex *tx = &texs[0];
    unsigned char *res = malloc((size_t)Wo * Ho * 4);
    for (int y = 0; y < Ho; y++)
        for (int x = 0; x < Wo; x++)
            memcpy(res + (y * Wo + x) * 4, tx->px + (((Ho - 1 - y) % tx->h) * tx->w + x % tx->w) * 4, 4);
    for (int t = 0; t < nlt; t++) {
        const float *a = &luv[t * 6], *b = &luv[t * 6 + 2], *c = &luv[t * 6 + 4];
        double m00 = b[0] - a[0], m01 = c[0] - a[0], m10 = b[1] - a[1], m11 = c[1] - a[1],
               det = m00 * m11 - m01 * m10;
        double mnx, mxx, mny, mxy;
        int x0, x1, y0, y1;
        if (det < 0 ? -det < 1e-12 : det < 1e-12)
            continue;
        uv_range(a[0], b[0], c[0], &mnx, &mxx);
        uv_range(a[1], b[1], c[1], &mny, &mxy);
        x0 = (int)floor(mnx * Wo);
        x1 = (int)ceil(mxx * Wo);
        y0 = (int)floor(mny * Ho);
        y1 = (int)ceil(mxy * Ho);
        if (y0 < 0)
            y0 = 0;
        if (y1 > Ho)
            y1 = Ho;
        for (int y = y0; y < y1; y++)
            for (int x = x0; x < x1; x++) {
                double qx = (x + 0.5) / Wo - a[0], qy = (y + 0.5) / Ho - a[1];
                double u = (m11 * qx - m01 * qy) / det, v = (-m10 * qx + m00 * qy) / det;
                V3 p, best = v3(0, 0, 0);
                double bd = 1e300;
                int bt = -1, xx = ((x % Wo) + Wo) % Wo, row = y;
                float bu, bv;
                if (u < -1e-6 || v < -1e-6 || u + v > 1 + 1e-6)
                    continue;
                p = add(add(mulf(lv[lt[t * 3]], (float)(1 - u - v)), mulf(lv[lt[t * 3 + 1]], (float)u)),
                        mulf(lv[lt[t * 3 + 2]], (float)v));
                if (FLAGS & FLAG_RAYCAST) {
                    V3 nn = norm(cross(sub(lv[lt[t * 3 + 1]], lv[lt[t * 3]]),
                                       sub(lv[lt[t * 3 + 2]], lv[lt[t * 3]])));
                    float lam;
                    int hh = raycast(add(p, mulf(nn, EXT)), mulf(nn, -1.0f), &lam);
                    if (hh >= 0) {
                        bt = hh;
                        best = add(add(p, mulf(nn, EXT)), mulf(nn, -lam));
                        bd = 0;
                    }
                }
                if (bt < 0)
                    for (int h = 0; h < nht; h++) {
                        V3 A = hv[ht[h * 3]], ab = sub(hv[ht[h * 3 + 1]], A), ac = sub(hv[ht[h * 3 + 2]], A),
                           ap = sub(p, A), n = cross(ab, ac);
                        double nn = (double)dot(n, n), pd, d00, d01, d11, d20, d21, den, vv, ww;
                        V3 q;
                        if (nn < 1e-24)
                            continue;
                        pd = dot(ap, n) / nn;
                        q = sub(p, mulf(n, (float)pd));
                        V3 v2 = sub(q, A);
                        d00 = dot(ab, ab);
                        d01 = dot(ab, ac);
                        d11 = dot(ac, ac);
                        d20 = dot(v2, ab);
                        d21 = dot(v2, ac);
                        den = d00 * d11 - d01 * d01;
                        if (den == 0.0)
                            continue;
                        vv = (d11 * d20 - d01 * d21) / den;
                        ww = (d00 * d21 - d01 * d20) / den;
                        if (vv < -1e-6 || ww < -1e-6 || 1 - vv - ww < -1e-6)
                            continue;
                        double dd = pd * pd * nn;
                        if (dd < bd) {
                            bd = dd;
                            bt = h;
                            best = q;
                        }
                    }
                if (bt < 0)
                    bt = closest_tri(p, &best, &bd);
                if (bt < 0)
                    continue;
                bary_of(bt, best, &bu, &bv);
                int ia = ht[bt * 3], ib = ht[bt * 3 + 1], ic = ht[bt * 3 + 2];
                float bw = 1.0f - bu - bv;
                float su = bu * huv[ia * 2] + bv * huv[ib * 2] + bw * huv[ic * 2];
                float sv = bu * huv[ia * 2 + 1] + bv * huv[ib * 2 + 1] + bw * huv[ic * 2 + 1];
                int sx = (int)floorf(su * tx->w), sy = (int)floorf(sv * tx->h);
                sx %= tx->w;
                if (sx < 0)
                    sx += tx->w;
                sy %= tx->h;
                if (sy < 0)
                    sy += tx->h;
                memcpy(res + (row * Wo + xx) * 4, tx->px + (sy * tx->w + sx) * 4, 4);
            }
    }
    write_file(NULL, res, (size_t)Wo * Ho * 4);
    free(res);
}

static void run_nearmap(void)
{
    int Wo = W, Ho = MARGIN, cnt = 0, cap = 4096;
    float *rec = malloc(sizeof(float) * 5 * cap);
    for (int t = 0; t < nlt; t++) {
        const float *a = &luv[t * 6], *b = &luv[t * 6 + 2], *c = &luv[t * 6 + 4];
        double m[3][3] = { { a[0], b[0], c[0] }, { a[1], b[1], c[1] }, { 1, 1, 1 } }, inv[3][3], det;
        double mnx, mxx, mny, mxy;
        int x0, x1, y0, y1;
        V3 P0 = lv[lt[t * 3]], P1 = lv[lt[t * 3 + 1]], P2 = lv[lt[t * 3 + 2]];
        det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) -
              m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) +
              m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if (det < 0 ? -det < 1e-12 : det < 1e-12)
            continue;
        inv[0][0] = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det;
        inv[0][1] = (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det;
        inv[0][2] = (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det;
        inv[1][0] = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det;
        inv[1][1] = (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det;
        inv[1][2] = (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det;
        inv[2][0] = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det;
        inv[2][1] = (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det;
        inv[2][2] = (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det;
        uv_range(a[0], b[0], c[0], &mnx, &mxx);
        uv_range(a[1], b[1], c[1], &mny, &mxy);
        x0 = (int)floor(mnx * Wo);
        x1 = (int)ceil(mxx * Wo);
        y0 = (int)floor((1 - mxy) * Ho);
        y1 = (int)ceil((1 - mny) * Ho);
        if (x0 < 0)
            x0 = 0;
        if (y0 < 0)
            y0 = 0;
        if (x1 > Wo)
            x1 = Wo;
        if (y1 > Ho)
            y1 = Ho;
        for (int y = y0; y < y1; y++)
            for (int x = x0; x < x1; x++) {
                double q[3] = { (x + 0.5) / Wo, 1 - (y + 0.5) / Ho, 1 }, bb[3], best = 1e300,
                       bw[3] = { 0, 0, 0 };
                int bt = -1;
                V3 p;
                for (int i = 0; i < 3; i++)
                    bb[i] = inv[i][0] * q[0] + inv[i][1] * q[1] + inv[i][2] * q[2];
                if (bb[0] < -1e-5 || bb[1] < -1e-5 || bb[2] < -1e-5)
                    continue;
                p = add(add(mulf(P0, (float)bb[0]), mulf(P1, (float)bb[1])), mulf(P2, (float)bb[2]));
                for (int h = 0; h < nht; h++) {
                    double A[3] = { hv[ht[h * 3]].x, hv[ht[h * 3]].y, hv[ht[h * 3]].z };
                    double B[3] = { hv[ht[h * 3 + 1]].x, hv[ht[h * 3 + 1]].y, hv[ht[h * 3 + 1]].z };
                    double C[3] = { hv[ht[h * 3 + 2]].x, hv[ht[h * 3 + 2]].y, hv[ht[h * 3 + 2]].z };
                    double pp[3] = { p.x, p.y, p.z }, ab[3], ac[3], ap[3], aa = 0, bq = 0, cc = 0, d = 0,
                           e = 0, den, u, v, dist, w3[3];
                    for (int k = 0; k < 3; k++) {
                        ab[k] = B[k] - A[k];
                        ac[k] = C[k] - A[k];
                        ap[k] = pp[k] - A[k];
                    }
                    for (int k = 0; k < 3; k++) {
                        aa += ab[k] * ab[k];
                        bq += ab[k] * ac[k];
                        cc += ac[k] * ac[k];
                        d += ap[k] * ab[k];
                        e += ap[k] * ac[k];
                    }
                    den = aa * cc - bq * bq;
                    if ((den < 0 ? -den : den) < 1e-12)
                        den = 1;
                    u = (cc * d - bq * e) / den;
                    v = (aa * e - bq * d) / den;
                    dist = 1e308;
                    w3[0] = 1 - u - v;
                    w3[1] = u;
                    w3[2] = v;
                    if (u >= 0 && v >= 0 && u + v <= 1) {
                        dist = 0;
                        for (int k = 0; k < 3; k++) {
                            double qq = A[k] + ab[k] * u + ac[k] * v - pp[k];
                            dist += qq * qq;
                        }
                    }
                    const double *Vt[3] = { A, B, C };
                    int ei[3][2] = { { 0, 1 }, { 1, 2 }, { 2, 0 } };
                    for (int eg = 0; eg < 3; eg++) {
                        const double *S = Vt[ei[eg][0]], *T = Vt[ei[eg][1]];
                        double ed[3], nn = 0, sp = 0, sf, dd = 0;
                        for (int k = 0; k < 3; k++) {
                            ed[k] = T[k] - S[k];
                            nn += ed[k] * ed[k];
                            sp += (pp[k] - S[k]) * ed[k];
                        }
                        sf = sp / (nn > 0 ? nn : 1);
                        if (sf < 0)
                            sf = 0;
                        if (sf > 1)
                            sf = 1;
                        for (int k = 0; k < 3; k++) {
                            double qq = S[k] + ed[k] * sf - pp[k];
                            dd += qq * qq;
                        }
                        if (dd < dist) {
                            dist = dd;
                            w3[0] = w3[1] = w3[2] = 0;
                            w3[ei[eg][0]] = 1 - sf;
                            w3[ei[eg][1]] = sf;
                        }
                    }
                    if (dist < best) {
                        best = dist;
                        bt = h;
                        bw[0] = w3[0];
                        bw[1] = w3[1];
                        bw[2] = w3[2];
                    }
                }
                if (bt < 0)
                    continue;
                if (cnt == cap) {
                    cap *= 2;
                    rec = realloc(rec, sizeof(float) * 5 * cap);
                }
                rec[cnt * 5] = (float)(y * Wo + x);
                rec[cnt * 5 + 1] = (float)bt;
                rec[cnt * 5 + 2] = (float)bw[0];
                rec[cnt * 5 + 3] = (float)bw[1];
                rec[cnt * 5 + 4] = (float)bw[2];
                cnt++;
            }
    }
    write_file(NULL, &cnt, 4);
    write_file(NULL, rec, sizeof(float) * (size_t)cnt * 5);
    free(rec);
}

static void run_pnear(void)
{
    float md2 = EXT * EXT;
    for (int i = 0; i < nlv; i++) {
        V3 p = lv[i], best = v3(0, 0, 0);
        double bd = 1e300;
        int bt = closest_tri(p, &best, &bd);
        float rec[4];
        rec[0] = -1.0f;
        rec[1] = rec[2] = rec[3] = 0.0f;
        if (bt >= 0 && bd <= md2) {
            V3 a = hv[ht[bt * 3]], v0 = sub(hv[ht[bt * 3 + 1]], a), v1 = sub(hv[ht[bt * 3 + 2]], a),
               v2 = sub(best, a);
            double d00 = dot(v0, v0), d01 = dot(v0, v1), d11 = dot(v1, v1), d20 = dot(v2, v0),
                   d21 = dot(v2, v1);
            double dd = d00 * d11 - d01 * d01;
            if (dd < 0 ? -dd >= 1e-12 : dd >= 1e-12) {
                rec[0] = (float)bt;
                rec[1] = (float)((d11 * d20 - d01 * d21) / dd);
                rec[2] = (float)((d00 * d21 - d01 * d20) / dd);
            }
        }
        write_file(NULL, rec, sizeof rec);
    }
}

static void run_bake(void)
{
    pix = malloc(sizeof(Pix) * W * H);
    for (int i = 0; i < W * H; i++)
        pix[i].tri = -1;
    rast_mode = 0;
    for (int t = 0; t < nlt; t++) {
        float a[2], b[2], c[2], A;
        uv_xy(&luv[t * 6], a);
        uv_xy(&luv[t * 6 + 2], b);
        uv_xy(&luv[t * 6 + 4], c);
        A = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if (fabsf(A) > 1.1920928955078125e-07f) {
            A = 0.5f / A;
            cur_d[0] = (b[1] - c[1]) * A;
            cur_d[2] = (c[1] - a[1]) * A;
            cur_d[1] = (c[0] - b[0]) * A;
            cur_d[3] = (a[0] - c[0]) * A;
        } else
            cur_d[0] = cur_d[1] = cur_d[2] = cur_d[3] = 0.0f;
        cur_tri = t;
        scanconvert(a, b, c);
    }
    img = calloc((size_t)W * H * 4, 1);
    mask = calloc((size_t)W * H, 1);
    for (int i = 0; i < W * H; i++)
        img[i * 4 + 3] = 255;
    for (int i = 0; i < W * H; i++) {
        Pix *p = &pix[i];
        int t = p->tri;
        V3 P0, P1, P2, co, dir, org, rd, hnorm, dxco, dyco, duco, dvco, tmp;
        float u, v, lam, acc[4] = { 0, 0, 0, 0 };
        int hit;
        if (t < 0)
            continue;
        P0 = lv[lt[t * 3]];
        P1 = lv[lt[t * 3 + 1]];
        P2 = lv[lt[t * 3 + 2]];
        u = p->u;
        v = p->v;
        co = add(add(mulf(P0, u), mulf(P1, v)), mulf(P2, 1.0f - u - v));
        dir = norm(cross(sub(P1, P0), sub(P2, P0)));
        org = add(co, mulf(dir, EXT));
        rd = mulf(dir, -1.0f);
        hit = raycast(org, rd, &lam);
        if (hit < 0)
            continue;
        V3 h0 = hv[ht[hit * 3]], h1 = hv[ht[hit * 3 + 1]], h2 = hv[ht[hit * 3 + 2]];
        V3 hco = add(org, mulf(rd, lam));
        float hit_u, hit_v, dxu, dxv, dyu, dyv;
        int a1, a2;
        float n0, n1, n2;
        hnorm = norm(cross(sub(h1, h0), sub(h2, h0)));
        duco = sub(P0, P2);
        dvco = sub(P1, P2);
        dxco = add(mulf(duco, p->dudx), mulf(dvco, p->dvdx));
        dyco = add(mulf(duco, p->dudy), mulf(dvco, p->dvdy));
        tmp = mulf(rd, 1.0f / dot(rd, hnorm));
        dxco = add(dxco, mulf(tmp, -dot(dxco, hnorm)));
        dyco = add(dyco, mulf(tmp, -dot(dyco, hnorm)));
        n0 = fabsf(hnorm.x);
        n1 = fabsf(hnorm.y);
        n2 = fabsf(hnorm.z);
        if (n2 >= n0 && n2 >= n1) {
            a1 = 0;
            a2 = 1;
        } else if (n1 >= n0 && n1 >= n2) {
            a1 = 0;
            a2 = 2;
        } else {
            a1 = 1;
            a2 = 2;
        }
        {
            float c0[3] = { h0.x, h0.y, h0.z }, c1[3] = { h1.x, h1.y, h1.z }, c2[3] = { h2.x, h2.y, h2.z };
            float cc[3] = { hco.x, hco.y, hco.z }, dx[3] = { dxco.x, dxco.y, dxco.z },
                  dy[3] = { dyco.x, dyco.y, dyco.z };
            float t00 = c2[a1] - c0[a1], t01 = c2[a2] - c0[a2], t10 = c2[a1] - c1[a1], t11 = c2[a2] - c1[a2];
            float det = t00 * t11 - t10 * t01;
            det = det != 0.0f ? 1.0f / det : 0.0f;
            t00 *= det;
            t01 *= det;
            t10 *= det;
            t11 *= det;
            hit_u = (c2[a1] - cc[a1]) * t11 - (c2[a2] - cc[a2]) * t10;
            hit_v = (c2[a2] - cc[a2]) * t00 - (c2[a1] - cc[a1]) * t01;
            dxu = dx[a1] * t11 - dx[a2] * t10;
            dxv = dx[a2] * t00 - dx[a1] * t01;
            dyu = dy[a1] * t11 - dy[a2] * t10;
            dyv = dy[a2] * t00 - dy[a1] * t01;
        }
        float cu = hit_v, cv = 1.0f - hit_u - hit_v;
        float cdudx = dxv, cdudy = dyv, cdvdx = -dxu - dxv, cdvdy = -dyu - dyv;
        if ((cu == 0.0f || cu == 1.0f) && (cv == 0.0f || cv == 1.0f)) {
            cu += 1e-4f * (0.5f - cu);
            cv += 1e-4f * (0.5f - cv);
        }
        float reg[4];
        int have_reg = 0;
        for (int s = 0; s < NS; s++) {
            float su = cu, sv = cv, col[4];
            const float *ap = (FLAGS & FLAG_AA_TABLE) ? aa_off + ((long)i * NS + s) * 2 : aa_off + s * 2;
            float fx = ap[0], fy = ap[1];
            if (fx != 0.0f || fy != 0.0f) {
                float ru = cu + cdudx * fx + cdudy * fy, rv = cv + cdvdx * fx + cdvdy * fy;
                if (ru > 0.0f && rv > 0.0f && ru + rv < 1.0f) {
                    su = ru;
                    sv = rv;
                } else {
                    float racc[4] = { 0, 0, 0, 0 };
                    int rn = 0;
                    if (!have_reg) {
                        for (int gy = 0; gy < 4; gy++)
                            for (int gx = 0; gx < 4; gx++) {
                                float ox = (gx - 1.5f) * 0.25f, oy = (gy - 1.5f) * 0.25f;
                                float qu = cu + cdudx * ox + cdudy * oy, qv = cv + cdvdx * ox + cdvdy * oy;
                                if (qu > 0.0f && qv > 0.0f && qu + qv < 1.0f) {
                                    float c2[4];
                                    shade(hit, 1.0f - qu - qv, qu, 1, c2);
                                    for (int k = 0; k < 4; k++)
                                        racc[k] += c2[k];
                                    rn++;
                                }
                            }
                        if (rn)
                            for (int k = 0; k < 4; k++)
                                reg[k] = racc[k] / rn;
                        else
                            shade(hit, 1.0f - cu - cv, cu, 1, reg);
                        have_reg = 1;
                    }
                    for (int k = 0; k < 4; k++)
                        acc[k] += reg[k];
                    continue;
                }
            }
            shade(hit, 1.0f - su - sv, su, 1, col);
            for (int k = 0; k < 4; k++)
                acc[k] += col[k];
        }
        for (int c = 0; c < 3; c++)
            img[i * 4 + c] = (unsigned char)to_byte_lin(acc[c] / (float)NS);
        mask[i] = 1;
    }
    if (MARGIN > 0)
        margin_adjacent();
    for (int i = 0; i < W * H; i++)
        img[i * 4 + 3] = 255;
    write_file(NULL, img, (size_t)W * H * 4);
}

static void run(const unsigned char *input, size_t input_size)
{
    buf = input;
    (void)input_size;
    bpos = 4;
    W = rd_i();
    H = rd_i();
    MARGIN = rd_i();
    FLAGS = rd_i();
    EXT = rd_f();
    NS = rd_i();
    long na = (FLAGS & FLAG_AA_TABLE) ? (long)W * H * NS * 2 : NS * 2;
    aa_off = malloc(sizeof(float) * (na + 2));
    for (long i = 0; i < na; i++)
        aa_off[i] = rd_f();
    nlv = rd_i();
    lv = malloc(sizeof(V3) * (nlv + 1));
    for (int i = 0; i < nlv; i++) {
        float x = rd_f(), y = rd_f(), z = rd_f();
        lv[i] = v3(x, y, z);
    }
    nlt = rd_i();
    lt = malloc(sizeof(int) * 3 * (nlt + 1));
    luv = malloc(sizeof(float) * 6 * (nlt + 1));
    lpoly = malloc(sizeof(int) * (nlt + 1));
    for (int i = 0; i < nlt; i++) {
        for (int k = 0; k < 3; k++)
            lt[i * 3 + k] = rd_i();
        for (int k = 0; k < 6; k++)
            luv[i * 6 + k] = rd_f();
        lpoly[i] = rd_i();
    }
    npoly = rd_i();
    pstart = malloc(sizeof(int) * (npoly + 1));
    pcount = malloc(sizeof(int) * (npoly + 1));
    int nl = rd_i();
    ploopuv = malloc(sizeof(float) * 2 * (nl + 1));
    loop_adj = malloc(sizeof(int) * (nl + 1));
    loop_poly = malloc(sizeof(int) * (nl + 1));
    for (int p = 0; p < npoly; p++) {
        pstart[p] = rd_i();
        pcount[p] = rd_i();
    }
    for (int l = 0; l < nl; l++) {
        rd_i();
        ploopuv[l * 2] = rd_f();
        ploopuv[l * 2 + 1] = rd_f();
        loop_adj[l] = rd_i();
    }
    for (int p = 0; p < npoly; p++)
        for (int l = pstart[p]; l < pstart[p] + pcount[p]; l++)
            loop_poly[l] = p;
    nhv = rd_i();
    hv = malloc(sizeof(V3) * (nhv + 1));
    huv = malloc(sizeof(float) * 2 * (nhv + 1));
    hcol = malloc(sizeof(float) * 4 * (nhv + 1));
    for (int i = 0; i < nhv; i++) {
        float x = rd_f(), y = rd_f(), z = rd_f();
        hv[i] = v3(x, y, z);
    }
    for (int i = 0; i < nhv * 2; i++)
        huv[i] = rd_f();
    for (int i = 0; i < nhv * 4; i++) {
        float v = rd_f();
        int bq = (int)(v * 255.0f + 0.5f);
        if (bq < 0)
            bq = 0;
        if (bq > 255)
            bq = 255;
        hcol[i] = (i & 3) == 3 ? v : vq[bq];
    }
    hnrm = malloc(sizeof(float) * 3 * (nhv + 1));
    for (int i = 0; i < nhv * 3; i++)
        hnrm[i] = rd_f();
    nht = rd_i();
    ht = malloc(sizeof(int) * 3 * (nht + 1));
    hmat = malloc(sizeof(int) * (nht + 1));
    for (int i = 0; i < nht; i++) {
        for (int k = 0; k < 3; k++)
            ht[i * 3 + k] = rd_i();
        hmat[i] = rd_i();
    }
    nmat = rd_i();
    mats = malloc(sizeof(Mat) * (nmat + 1));
    for (int i = 0; i < nmat; i++) {
        Mat *m = &mats[i];
        m->tex = rd_i();
        m->wrap_s = rd_i();
        m->wrap_t = rd_i();
        m->alpha_mode = rd_i();
        m->has_xf = rd_i();
        for (int k = 0; k < 4; k++)
            m->factor[k] = rd_f();
        m->cutoff = rd_f();
        for (int k = 0; k < 4; k++)
            m->xf[k] = rd_f();
    }
    ntex = rd_i();
    texs = malloc(sizeof(Tex) * (ntex + 1));
    for (int i = 0; i < ntex; i++) {
        texs[i].w = rd_i();
        texs[i].h = rd_i();
        texs[i].px = buf + bpos;
        bpos += (long)texs[i].w * texs[i].h * 4;
    }
    hpart = NULL;
    if (FLAGS & FLAG_PART_IDS) {
        hpart = malloc(sizeof(int) * (nht + 1));
        for (int i = 0; i < nht; i++)
            hpart[i] = rd_i();
    }
    nnodes = 0;
    nodes = malloc(sizeof(Node) * (2 * nht + 2));
    tri_idx = malloc(sizeof(int) * (nht + 1));
    tcent = malloc(sizeof(float) * 3 * (nht + 1));
    for (int t = 0; t < nht; t++) {
        V3 a = hv[ht[t * 3]], b = hv[ht[t * 3 + 1]], c = hv[ht[t * 3 + 2]];
        tri_idx[t] = t;
        tcent[t * 3] = (a.x + b.x + c.x) / 3.0f;
        tcent[t * 3 + 1] = (a.y + b.y + c.y) / 3.0f;
        tcent[t * 3 + 2] = (a.z + b.z + c.z) / 3.0f;
    }
    if (nht)
        build(0, nht);
    if (FLAGS & FLAG_PARTS) {
        run_parts();
        return;
    }
    if (FLAGS & FLAG_POINT_NEAR) {
        run_pnear();
        return;
    }
    if (FLAGS & FLAG_NEAR_MAP) {
        run_nearmap();
        return;
    }
    if (FLAGS & FLAG_REPROJECT) {
        run_reproj();
        return;
    }
    run_bake();
}

#ifdef TEXTURES_BAKE_CORE_LIBRARY
void textures_bake_core_set_arena(unsigned char *arena, size_t arena_size)
{
#ifdef TEXTURES_BAKE_CORE_ARENA
    core_arena = arena;
    core_arena_size = arena_size;
    core_arena_used = 0;
    core_arena_failed = 0;
#else
    (void)arena;
    (void)arena_size;
#endif
    result_buf = NULL;
    result_len = result_cap = 0;
    result_failed = 0;
}

int textures_bake_core_run(const unsigned char *input, size_t input_size)
{
    if (!input || input_size < 24)
        return 1;
    init_tables();
    result_len = 0;
    result_failed = 0;
    run(input, input_size);
#ifdef TEXTURES_BAKE_CORE_ARENA
    return result_failed || core_arena_failed ? 2 : 0;
#else
    return result_failed ? 2 : 0;
#endif
}

const unsigned char *textures_bake_core_output(void) { return result_buf; }
size_t textures_bake_core_output_len(void) { return result_len; }
void textures_bake_core_reset(void)
{
#ifndef TEXTURES_BAKE_CORE_ARENA
    free(result_buf);
#endif
    result_buf = NULL;
    result_len = result_cap = 0;
    result_failed = 0;
#ifdef TEXTURES_BAKE_CORE_ARENA
    core_arena_used = 0;
    core_arena_failed = 0;
#endif
}
#else
int main(int argc, char **argv)
{
    init_tables();
    if (argc == 4 && strcmp(argv[1], "-tev") == 0) {
        run_tev(argv[2], argv[3]);
        return 0;
    }
    if (argc != 3)
        return 1;
    load_input(argv[1]);
    native_output_path = argv[2];
    run(buf, (size_t)bpos);
    if (result_failed) {
        free(owned_buf);
        free(result_buf);
        return 2;
    }
    FILE *f = fopen(native_output_path, "wb");
    if (!f || fwrite(result_buf, 1, result_len, f) != result_len) {
        if (f)
            fclose(f);
        free(owned_buf);
        free(result_buf);
        return 2;
    }
    fclose(f);
    free(owned_buf);
    free(result_buf);
    return 0;
}
#endif
