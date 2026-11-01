#include "stage_extras.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

static char *g_type_names[256];
static int g_type_count;

void object_types_add(const char *name) {
    for (int i = 0; i < g_type_count; i++)
        if (!strcmp(g_type_names[i], name)) return;
    if (g_type_count == 256) die("too many object types");
    g_type_names[g_type_count++] = xstrdup(name);
}

int object_types_count(void) { return g_type_count; }
const char *object_type_name(int i) { return g_type_names[i]; }

static int object_type_index(const char *name, const char *path, int line) {
    for (int i = 0; i < g_type_count; i++)
        if (!strcmp(g_type_names[i], name)) return i;
    die("%s:%d: unknown object type '%s' (add it to object_types in assets.cfg)", path, line, name);
}

static float dot3(const float a[3], const float b[3]) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; }
static float len3(const float a[3]) { return sqrtf(dot3(a, a)); }

static void normalize3(float v[3]) {
    float m = len3(v);
    if (m > 0) {
        v[0] /= m;
        v[1] /= m;
        v[2] /= m;
    }
}

static void cross3(const float a[3], const float b[3], float out[3]) {
    float c[3] = { a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0] };
    memcpy(out, c, sizeof c);
}

static void rotate3(const float v[3], const float k[3], float a, float out[3]) {
    float c = cosf(a), s = sinf(a), kv[3], d = dot3(k, v);
    cross3(k, v, kv);
    float r[3];
    for (int i = 0; i < 3; i++) r[i] = v[i] * c + kv[i] * s + k[i] * d * (1 - c);
    memcpy(out, r, sizeof r);
}

static void floats(char **t, int n, float *out, const char *path, int line) {
    for (int i = 0; i < n; i++) {
        char *end;
        if (!t[i]) die("%s:%d: expected %d numbers", path, line, n);
        out[i] = strtof(t[i], &end);
        if (*end) die("%s:%d: bad number '%s'", path, line, t[i]);
    }
}

static void csv_floats(const char *s, int n, float *out, const char *path, int line) {
    for (int i = 0; i < n; i++) {
        char *end;
        out[i] = strtof(s, &end);
        if (end == s) die("%s:%d: bad vector '%s'", path, line, s);
        s = end;
        if (i + 1 < n) {
            if (*s != ',') die("%s:%d: bad vector", path, line);
            s++;
        }
    }
}

static int compare_gravity_priority(const void *x, const void *y, void *ctx) {
    (void)ctx;
    return ((const GravityDef *)y)->priority - ((const GravityDef *)x)->priority;
}

typedef struct {
    char *key[48], *val[48];
    int n;
} Opts;

static Opts split_opts(char **t, int first, int n) {
    Opts o = { { 0 }, { 0 }, 0 };
    for (int k = first; k < n; k++) {
        char *eq = strchr(t[k], '=');
        o.key[o.n] = t[k];
        o.val[o.n] = NULL;
        if (eq) {
            *eq = 0;
            o.val[o.n] = eq + 1;
        }
        o.n++;
    }
    return o;
}

static int arg_index(const char *key) {
    return key[0] == 'a' && key[1] >= '0' && key[1] <= '7' && !key[2] ? key[1] - '0' : -1;
}

typedef struct {
    float o[3], x[3], y[3], z[3], s[3];
} Frame;

static Frame read_frame(char **t, const char *path, int line) {
    float v[15];
    floats(t, 15, v, path, line);
    Frame f;
    memcpy(f.o, v, 12);
    memcpy(f.x, v + 3, 12);
    memcpy(f.y, v + 6, 12);
    memcpy(f.z, v + 9, 12);
    memcpy(f.s, v + 12, 12);
    normalize3(f.x);
    normalize3(f.y);
    normalize3(f.z);
    return f;
}

static float maxf3(const float s[3]) { return fmaxf(s[0], fmaxf(s[1], s[2])); }

#define GALAXY 256.0f

static void set_sector(GravityDef *g, const float side[3], const float axis[3], int degree) {

    if (degree < 0 || degree >= 360) {
        g->valid_cos = -1.0f;
        return;
    }
    float s[3] = { side[0], side[1], side[2] };
    float d = dot3(s, axis);
    for (int k = 0; k < 3; k++) s[k] -= axis[k] * d;
    normalize3(s);
    float half = (float)degree * 0.5f * 3.14159265f / 180.0f;
    rotate3(s, axis, half, g->ref_side);
    g->valid_cos = cosf(half);
}

static void galaxy_gravity(GravityDef *g, const char *kind, const Frame *f, const int a[3], const char *path,
                           int line) {
    memcpy(g->centre, f->o, 12);
    memcpy(g->axis[0], f->x, 12);
    memcpy(g->axis[1], f->y, 12);
    memcpy(g->axis[2], f->z, 12);
    g->base = 2000.0f / GALAXY;
    g->valid_cos = -1.0f;
    if (!strcmp(kind, "gpoint")) {
        g->kind = GRAV_POINT;
        if (g->distant < 0) g->distant = 500.0f / GALAXY * f->s[0];
    } else if (!strcmp(kind, "gcube") || !strcmp(kind, "gplanebox")) {

        for (int k = 0; k < 3; k++) g->extent[k] = 500.0f / GALAXY * f->s[k];
        for (int k = 0; k < 3; k++) g->centre[k] = f->o[k] + f->y[k] * g->extent[1];
        if (kind[1] == 'c') {
            g->kind = GRAV_CUBE;
            g->faces = 0;
            for (int i = 0; i < 3; i++) {
                int v = a[i] == -1 ? 3 : a[i];
                if (v & 1) g->faces |= 1 << (2 * i);
                if (v & 2) g->faces |= 2 << (2 * i);
            }
        } else {
            g->kind = GRAV_PLANE_BOX;
            if (a[0] >= 0) g->base = (float)a[0] / GALAXY;
            g->faces = a[1] >= 0 && a[1] <= 2 ? a[1] : 3;
        }
    } else if (!strcmp(kind, "gplane")) {
        g->kind = GRAV_PLANE;
    } else if (!strcmp(kind, "gplanecyl")) {
        g->kind = GRAV_PLANE_CYLINDER;
        g->extent[0] = 500.0f / GALAXY * f->s[0];
        g->extent[1] = 500.0f / GALAXY * f->s[1];
        if (a[0] >= 0) g->base = (float)a[0] / GALAXY;
    } else if (!strcmp(kind, "gdisk")) {
        g->kind = GRAV_DISK;
        g->extent[0] = 500.0f / GALAXY * maxf3(f->s);
        if (a[0] != 0) g->flags |= GRAVF_BOTH_SIDES;
        if (a[1] != 0) g->flags |= GRAVF_EDGE;
        set_sector(g, f->x, f->y, a[2]);
    } else if (!strcmp(kind, "gdisktorus")) {
        g->kind = GRAV_DISK_TORUS;
        g->extent[0] = 500.0f / GALAXY * maxf3(f->s);
        g->extent[1] = a[2] > 0 ? (float)a[2] / GALAXY : 0.0f;
        if (a[0] != 0) g->flags |= GRAVF_BOTH_SIDES;
        g->faces = a[1] >= 0 && a[1] <= 2 ? a[1] : 3;
    } else if (!strcmp(kind, "gwire")) {

        g->kind = GRAV_WIRE;
        g->wire_count = (a[0] >= 0 ? a[0] : 20) + 2;
    } else if (!strcmp(kind, "gsegment")) {

        g->kind = GRAV_SEGMENT;
        g->extent[1] = 1000.0f / GALAXY * f->s[1];
        int e = a[0];
        if (e != 0 && e != 2) g->flags |= GRAVF_EDGE0;
        if (e != 0 && e != 1) g->flags |= GRAVF_EDGE1;
        set_sector(g, f->x, f->y, a[1]);
    } else {
        die("%s:%d: gravity kind '%s' is not supported", path, line, kind);
    }
}

static void legacy_gravity(GravityDef *g, const char *kind, char **a, int n, const char *path, int line) {
    g->flags |= GRAVF_LEGACY;
    g->valid_cos = -1.0f;
    g->range = -1.0f;
    if (!strcmp(kind, "point")) {
        float v[5];
        floats(a, 5, v, path, line);
        g->kind = GRAV_POINT;
        memcpy(g->centre, v, 12);
        g->distant = v[3];
        g->range = v[4];
        if (n > 8 && !strcmp(a[5], "inverse")) g->flags |= GRAVF_INVERSE;
        g->axis[1][2] = 1;
        g->axis[0][0] = 1;
        g->axis[2][1] = 1;
    } else if (!strcmp(kind, "parallel")) {
        float v[12];
        floats(a, 12, v, path, line);
        g->kind = GRAV_PLANE_BOX;
        memcpy(g->centre, v, 12);
        memcpy(g->axis[1], v + 3, 12);
        memcpy(g->axis[0], v + 6, 12);
        normalize3(g->axis[0]);
        normalize3(g->axis[1]);
        cross3(g->axis[1], g->axis[0], g->axis[2]);
        g->extent[0] = v[9];
        g->extent[2] = v[10];
        g->extent[1] = v[11];
        g->faces = 3;
        g->base = 0;
    } else if (!strcmp(kind, "cylinder")) {
        float v[8];
        floats(a, 8, v, path, line);
        g->kind = GRAV_PLANE_CYLINDER;
        memcpy(g->centre, v, 12);
        memcpy(g->axis[1], v + 3, 12);
        normalize3(g->axis[1]);
        g->extent[0] = v[6];
        g->extent[1] = v[7];
        g->base = 0;
    } else if (!strcmp(kind, "segment")) {
        float v[8];
        floats(a, 8, v, path, line);
        g->kind = GRAV_SEGMENT;
        memcpy(g->centre, v, 12);
        for (int k = 0; k < 3; k++) g->axis[1][k] = v[3 + k] - v[k];
        g->extent[1] = len3(g->axis[1]);
        normalize3(g->axis[1]);
        g->distant = v[6];
        g->range = v[7];
        g->flags |= GRAVF_EDGE0 | GRAVF_EDGE1;
    } else if (!strcmp(kind, "disk")) {
        float v[8];
        floats(a, 8, v, path, line);
        g->kind = GRAV_DISK;
        memcpy(g->centre, v, 12);
        memcpy(g->axis[1], v + 3, 12);
        normalize3(g->axis[1]);
        g->extent[0] = v[6];
        g->range = v[7];
        g->flags |= GRAVF_BOTH_SIDES | GRAVF_EDGE;
    } else {
        die("%s:%d: unknown gravity kind '%s'", path, line, kind);
    }
}

static int camera_type(const char *s, const char *path, int line) {
    static const char *names[CAM_TYPE_COUNT] = {
        "CAM_TYPE_XZ_PARA", "CAM_TYPE_WONDER_PLANET", "CAM_TYPE_FOLLOW", "CAM_TYPE_POINT_FIX",
        "CAM_TYPE_EYEPOS_FIX", "CAM_TYPE_EYEPOS_FIX_THERE", "CAM_TYPE_CHARMED_FIX", "CAM_TYPE_TOWER",
        "CAM_TYPE_TOWER_POS", "CAM_TYPE_OBJ_PARALLEL", "CAM_TYPE_CUBE_PLANET", "CAM_TYPE_FRONT_AND_BACK",
        "CAM_TYPE_TRUNDLE", "CAM_TYPE_MEDIAN_PLANET", "CAM_TYPE_WATER_FOLLOW", "CAM_TYPE_SPIRAL_DEMO",
        "CAM_TYPE_CHARMED_VECREG_TOWER",
    };
    for (int i = 0; i < CAM_TYPE_COUNT; i++)
        if (!strcmp(names[i], s)) return i;
#ifdef SMG_ASSETC_MEMORY_FS
    smg_assetc_fprintf(stderr,
                       "%s:%d: camera type %s not supported, using CAM_TYPE_XZ_PARA (Galaxy's default)\n",
                       path, line, s);
#else
    fprintf(stderr, "%s:%d: camera type %s not supported, using CAM_TYPE_XZ_PARA (Galaxy's default)\n", path,
            line, s);
#endif
    return CAM_XZ_PARA;
}

typedef struct {
    Vector grav, objs, starts, rails, rpoints, areas, cameras, parts;
} Lists;

static void parse_file(const char *path, Lists *L, int is_galaxy, int *line_counter) {
    char *text = (char *)read_file(path, NULL);
    if (!text) return;
    int line_num = 0;
    RailDef *rail = NULL;
    for (char *line = text; line && *line;) {
        char *next = strchr(line, '\n');
        if (next) *next++ = 0;
        line_num++;
        char *hash = strchr(line, '#');
        if (hash) *hash = 0;
        char *t[64] = { 0 };
        int n = 0;
        for (char *tok = strtok(line, " \t\r"); tok && n < 63; tok = strtok(NULL, " \t\r")) t[n++] = tok;
        line = next;
        if (!n) continue;
        (*line_counter)++;
        if (!strcmp(t[0], "start")) {
            StartDef *s = vec_push(&L->starts);
            memset(s, 0, sizeof *s);
            float v[9];
            floats(t + 1, 9, v, path, line_num);
            memcpy(s->position, v, 12);
            memcpy(s->up, v + 3, 12);
            memcpy(s->front, v + 6, 12);
            normalize3(s->up);
            normalize3(s->front);
            s->scenarios = 0xFF;
            s->camera = -1;
            s->id = (int)L->starts.len - 1;
            Opts o = split_opts(t, 10, n);
            for (int k = 0; k < o.n; k++) {
                if (!o.val[k]) die("%s:%d: expected key=value", path, line_num);
                if (!strcmp(o.key[k], "id")) s->id = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "scen")) s->scenarios = (int)strtol(o.val[k], NULL, 16);
                else if (!strcmp(o.key[k], "zone")) s->zone = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "camera")) s->camera = atoi(o.val[k]);
                else die("%s:%d: unknown start key '%s'", path, line_num, o.key[k]);
            }
        } else if (!strcmp(t[0], "gravity") && n >= 3) {
            GravityDef *g = vec_push(&L->grav);
            memset(g, 0, sizeof *g);
            g->switch_a = g->switch_b = g->switch_appear = g->switch_sleep = -1;
            g->rail = -1;
            g->priority = atoi(t[2]);
            g->scenarios = 0xFF;
            if (t[1][0] == 'g') {
                Frame f = read_frame(t + 3, path, line_num);
                int a[3] = { -1, -1, -1 };
                g->range = -1;
                g->distant = -1;
                Opts o = split_opts(t, 18, n);
                for (int k = 0; k < o.n; k++) {
                    int ai = arg_index(o.key[k]);
                    if (ai >= 0 && ai < 3 && o.val[k]) a[ai] = atoi(o.val[k]);
                    else if (ai >= 3) continue;
                    else if (!strcmp(o.key[k], "range")) g->range = strtof(o.val[k], NULL);
                    else if (!strcmp(o.key[k], "distant")) g->distant = strtof(o.val[k], NULL);
                    else if (!strcmp(o.key[k], "inverse")) g->flags |= GRAVF_INVERSE;
                    else if (!strcmp(o.key[k], "light")) g->flags |= GRAVF_LIGHT;
                    else if (!strcmp(o.key[k], "scen")) g->scenarios = (int)strtol(o.val[k], NULL, 16);
                    else if (!strcmp(o.key[k], "swa")) g->switch_a = atoi(o.val[k]);
                    else if (!strcmp(o.key[k], "swb")) g->switch_b = atoi(o.val[k]);
                    else if (!strcmp(o.key[k], "swappear")) g->switch_appear = atoi(o.val[k]);
                    else if (!strcmp(o.key[k], "swsleep")) g->switch_sleep = atoi(o.val[k]);
                    else if (!strcmp(o.key[k], "rail")) g->rail = atoi(o.val[k]);
                    else die("%s:%d: unknown gravity key '%s'", path, line_num, o.key[k]);
                }
                galaxy_gravity(g, t[1], &f, a, path, line_num);
                if (g->distant < 0) g->distant = 0;
            } else {
                legacy_gravity(g, t[1], t + 3, n, path, line_num);
            }
            for (int k = 0; k < 3; k++) normalize3(g->axis[k]);
        } else if (!strcmp(t[0], "object") && n >= 11) {
            ObjectDef *o = vec_push(&L->objs);
            memset(o, 0, sizeof *o);
            o->type = object_type_index(t[1], path, line_num);
            o->scenarios = 0xFF;
            o->rail = -1;
            o->camera = -1;
            o->id = is_galaxy ? -1 : *line_counter;
            for (int k = 0; k < 8; k++) o->args[k] = k < 4 && !is_galaxy ? 0 : -1;
            o->scale[0] = o->scale[1] = o->scale[2] = 1.0f;
            for (int k = 0; k < 5; k++) o->sw[k] = -1;
            float v[9];
            floats(t + 2, 9, v, path, line_num);
            memcpy(o->position, v, 12);
            memcpy(o->up, v + 3, 12);
            memcpy(o->front, v + 6, 12);
            normalize3(o->up);
            normalize3(o->front);
            memcpy(o->target, o->position, 12);
            Opts op = split_opts(t, 11, n);
            for (int k = 0; k < op.n; k++) {
                const char *key = op.key[k], *val = op.val[k];
                if (!val) die("%s:%d: expected key=value, got '%s'", path, line_num, key);
                int ai = arg_index(key);
                if (!strcmp(key, "scen")) o->scenarios = (int)strtol(val, NULL, 16);
                else if (!strcmp(key, "flags")) o->flags = atoi(val);
                else if (!strcmp(key, "target")) csv_floats(val, 3, o->target, path, line_num);
                else if (ai >= 0) o->args[ai] = atoi(val);
                else if (!strcmp(key, "rail")) o->rail = atoi(val);
                else if (!strcmp(key, "camera")) o->camera = atoi(val);
                else if (!strcmp(key, "id")) o->id = atoi(val);
                else if (!strcmp(key, "zone")) o->zone = atoi(val);
                else if (!strcmp(key, "scale")) csv_floats(val, 3, o->scale, path, line_num);
                else if (!strcmp(key, "sw")) {
                    float v[5];
                    csv_floats(val, 5, v, path, line_num);
                    for (int s = 0; s < 5; s++) o->sw[s] = (int)v[s];
                }
                else die("%s:%d: unknown object key '%s'", path, line_num, key);
            }
            if (o->id < 0) o->id = (int)L->objs.len - 1;
        } else if (!strcmp(t[0], "rail") && n >= 2) {
            if (atoi(t[1]) != (int)L->rails.len)
                die("%s:%d: rails must be numbered in order", path, line_num);
            rail = vec_push(&L->rails);
            memset(rail, 0, sizeof *rail);
            for (int k = 0; k < 8; k++) rail->args[k] = -1;
            rail->first_point = (int)L->rpoints.len;
            Opts o = split_opts(t, 2, n);
            for (int k = 0; k < o.n; k++) {
                int ai = arg_index(o.key[k]);
                if (!strcmp(o.key[k], "closed")) rail->closed = 1;
                else if (ai >= 0 && o.val[k]) rail->args[ai] = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "zone") && o.val[k]) rail->zone = atoi(o.val[k]);
                else die("%s:%d: unknown rail key '%s'", path, line_num, o.key[k]);
            }
        } else if (!strcmp(t[0], "rpoint") && n >= 10) {
            if (!rail) die("%s:%d: rpoint without a rail", path, line_num);
            RailPointDef *p = vec_push(&L->rpoints);
            memset(p, 0, sizeof *p);
            float v[9];
            floats(t + 1, 9, v, path, line_num);
            memcpy(p->position, v, 12);
            memcpy(p->in, v + 3, 12);
            memcpy(p->out, v + 6, 12);
            for (int k = 0; k < 8; k++) p->args[k] = -1;
            Opts o = split_opts(t, 10, n);
            for (int k = 0; k < o.n; k++) {
                int ai = arg_index(o.key[k]);
                if (ai < 0 || !o.val[k]) die("%s:%d: unknown rpoint key '%s'", path, line_num, o.key[k]);
                p->args[ai] = atoi(o.val[k]);
            }
            rail->point_count++;
        } else if (!strcmp(t[0], "area") && n >= 18) {
            AreaDef *a = vec_push(&L->areas);
            memset(a, 0, sizeof *a);
            for (int k = 0; k < 8; k++) a->args[k] = -1;
            a->scenarios = 0xFF;
            a->camera = -1;
            a->sw = -1;
            if (!strcmp(t[1], "camera")) a->cls = AREA_CAMERA;
            else if (!strcmp(t[1], "death")) a->cls = AREA_DEATH;
            else if (!strcmp(t[1], "restart")) a->cls = AREA_RESTART;
            else if (!strcmp(t[1], "blackhole")) a->cls = AREA_BLACKHOLE;
            else if (!strcmp(t[1], "switch")) a->cls = AREA_SWITCH;
            else if (!strcmp(t[1], "tower")) a->cls = AREA_TOWER;
            else die("%s:%d: unknown area class '%s'", path, line_num, t[1]);
            Frame f = read_frame(t + 3, path, line_num);
            memcpy(a->centre, f.o, 12);
            memcpy(a->axis[0], f.x, 12);
            memcpy(a->axis[1], f.y, 12);
            memcpy(a->axis[2], f.z, 12);
            for (int k = 0; k < 3; k++) a->extent[k] = 500.0f / GALAXY * f.s[k];
            if (!strcmp(t[2], "cbox")) {
                a->shape = SHAPE_BOX;
            } else if (!strcmp(t[2], "box")) {
                a->shape = SHAPE_BOX;
                for (int k = 0; k < 3; k++) a->centre[k] += f.y[k] * a->extent[1];
            } else if (!strcmp(t[2], "sphere") || !strcmp(t[2], "bowl")) {
                a->shape = t[2][1] == 'p' ? SHAPE_SPHERE : SHAPE_BOWL;
                a->extent[1] = a->extent[2] = a->extent[0];
            } else if (!strcmp(t[2], "cylinder")) {
                a->shape = SHAPE_CYLINDER;
                a->extent[2] = 0;
            } else {
                die("%s:%d: unknown area shape '%s'", path, line_num, t[2]);
            }
            Opts o = split_opts(t, 18, n);
            for (int k = 0; k < o.n; k++) {
                int ai = arg_index(o.key[k]);
                if (!o.val[k]) die("%s:%d: expected key=value", path, line_num);
                if (ai >= 0) a->args[ai] = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "scen")) a->scenarios = (int)strtol(o.val[k], NULL, 16);
                else if (!strcmp(o.key[k], "zone")) a->zone = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "camera")) a->camera = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "sw")) a->sw = atoi(o.val[k]);
                else die("%s:%d: unknown area key '%s'", path, line_num, o.key[k]);
            }
            a->priority = a->cls == AREA_CAMERA ? a->args[2] : 0;
        } else if (!strcmp(t[0], "part") && n >= 3) {

            PartDef *pd = vec_push(&L->parts);
            memset(pd, 0, sizeof *pd);
            snprintf(pd->name, sizeof pd->name, "%s", t[1]);
            pd->sw = -1;
            Opts o = split_opts(t, 2, n);
            for (int k = 0; k < o.n; k++) {
                if (!o.val[k]) die("%s:%d: expected key=value", path, line_num);
                if (!strcmp(o.key[k], "sw")) pd->sw = atoi(o.val[k]);
                else if (!strcmp(o.key[k], "mode"))
                    pd->mode = !strcmp(o.val[k], "hide") ? PART_HIDE_WHEN_ON : PART_SHOW_WHEN_ON;
                else die("%s:%d: unknown part key '%s'", path, line_num, o.key[k]);
            }
            if (L->parts.len > STAGE_MAX_PARTS)
                die("%s:%d: at most %d parts", path, line_num, STAGE_MAX_PARTS);
        } else if (!strcmp(t[0], "camera") && n >= 3) {
            if (atoi(t[1]) != (int)L->cameras.len)
                die("%s:%d: cameras must be numbered in order", path, line_num);
            CameraDef *c = vec_push(&L->cameras);
            memset(c, 0, sizeof *c);
            c->type = camera_type(t[2], path, line_num);
            c->camint = 120;
            c->fovy = 45;
            c->angle_b = 0.3f;
            c->vpan = 1;
            c->upper = 0.3f;
            c->lower = 0.1f;
            c->uplay = 300.0f / GALAXY;
            c->lplay = 800.0f / GALAXY;
            c->up[1] = 1;
            c->woffset[2] = 100.0f / GALAXY;
            c->zone_up[2] = 1;
            c->camendint = 120;
            Opts o = split_opts(t, 3, n);
            for (int k = 0; k < o.n; k++) {
                const char *key = o.key[k], *val = o.val[k];
                if (!val) die("%s:%d: expected key=value", path, line_num);
                if (!strcmp(key, "dist")) c->dist = strtof(val, NULL);
                else if (!strcmp(key, "offset") || !strcmp(key, "local"))
                    csv_floats(val, 3, c->offset, path, line_num);
                else if (!strcmp(key, "wpoint") || !strcmp(key, "base"))
                    csv_floats(val, 3, c->wpoint, path, line_num);
                else if (!strcmp(key, "axis")) csv_floats(val, 3, c->axis, path, line_num);
                else if (!strcmp(key, "reg")) {
                    char name[64];
                    const char *comma = strchr(val, ',');
                    if (!comma || comma - val >= (ptrdiff_t)sizeof(name))
                        die("%s:%d: invalid camera register", path, line_num);
                    memcpy(name, val, (size_t)(comma - val));
                    name[comma - val] = 0;
                    c->reg_type = object_type_index(name, path, line_num);
                    if (c->reg_type <= 0 || c->reg_type > 255)
                        die("%s:%d: camera register type must fit in a byte", path, line_num);
                    c->reg_id = atoi(comma + 1);
                }
                else if (!strcmp(key, "up")) csv_floats(val, 3, c->up, path, line_num);
                else if (!strcmp(key, "woffset")) csv_floats(val, 3, c->woffset, path, line_num);
                else if (!strcmp(key, "zup")) csv_floats(val, 3, c->zone_up, path, line_num);
                else if (!strcmp(key, "lup") || !strcmp(key, "zx"))
                    csv_floats(val, 3, c->extra, path, line_num);
                else if (!strcmp(key, "loffset")) {
                    float v[2];
                    csv_floats(val, 2, v, path, line_num);
                    c->loffset = v[0];
                    c->loffsetv = v[1];
                } else if (!strcmp(key, "angleA")) c->angle_a = strtof(val, NULL);
                else if (!strcmp(key, "angleB")) c->angle_b = strtof(val, NULL);
                else if (!strcmp(key, "roll")) c->roll = strtof(val, NULL);
                else if (!strcmp(key, "fovy")) c->fovy = strtof(val, NULL);
                else if (!strcmp(key, "camint")) c->camint = atoi(val);
                else if (!strcmp(key, "evfrm")) c->evfrm = atoi(val);
                else if (!strcmp(key, "num1")) c->num1 = atoi(val);
                else if (!strcmp(key, "num2")) c->num2 = atoi(val);
                else if (!strcmp(key, "rate")) continue;
                else if (!strcmp(key, "flags")) c->flags = atoi(val);
                else if (!strcmp(key, "camendint")) c->camendint = atoi(val);
                else if (!strcmp(key, "vpan")) {
                    float v[5];
                    csv_floats(val, 5, v, path, line_num);
                    c->vpan = (int)v[0];
                    c->upper = v[1];
                    c->lower = v[2];
                    c->uplay = v[3];
                    c->lplay = v[4];
                } else die("%s:%d: unknown camera key '%s'", path, line_num, key);
            }
        } else {
            die("%s:%d: unexpected '%s'", path, line_num, t[0]);
        }
    }
    free(text);
}

static void wire_sample(const StageExtras *e, GravityDef *g, const char *path);

StageExtras stage_extras_load(const char *stage_path, const char *galaxy_path) {
    StageExtras e = { 0 };
    Lists st = { VEC(GravityDef), VEC(ObjectDef), VEC(StartDef), VEC(RailDef), VEC(RailPointDef),
                 VEC(AreaDef), VEC(CameraDef), VEC(PartDef) };
    Lists gx = st;
    int counter = 0;
    parse_file(stage_path, &st, 0, &counter);
    char *probe = (char *)read_file(galaxy_path, NULL);
    if (probe) {
        free(probe);
        e.galaxy = 1;
        parse_file(galaxy_path, &gx, 1, &counter);

        if (gx.starts.len) {
            vec_free(&st.starts);
            st.starts = gx.starts;
        }
        if (gx.grav.len) {
            vec_free(&st.grav);
            st.grav = gx.grav;
        }
        if (gx.objs.len) {
            vec_free(&st.objs);
            st.objs = gx.objs;
        }
        st.rails = gx.rails;
        st.rpoints = gx.rpoints;
        st.areas = gx.areas;
        st.cameras = gx.cameras;
        st.parts = gx.parts;
    }
    stable_sort(st.grav.data, st.grav.len, sizeof(GravityDef), compare_gravity_priority, NULL);
    e.gravities = st.grav.data;
    e.gravity_count = (int)st.grav.len;
    e.objects = st.objs.data;
    e.object_count = (int)st.objs.len;
    e.starts = st.starts.data;
    e.start_count = (int)st.starts.len;
    e.rails = st.rails.data;
    e.rail_count = (int)st.rails.len;
    e.rail_points = st.rpoints.data;
    e.rail_point_count = (int)st.rpoints.len;
    e.areas = st.areas.data;
    e.area_count = (int)st.areas.len;
    e.cameras = st.cameras.data;
    e.camera_count = (int)st.cameras.len;
    e.parts = st.parts.data;
    e.part_count = (int)st.parts.len;
    for (int i = 0; i < e.part_count; i++) e.part_names[i] = e.parts[i].name;
    for (int i = 0; i < e.gravity_count; i++) wire_sample(&e, &e.gravities[i], galaxy_path);
    for (int i = 0; i < e.object_count; i++) {
        const ObjectDef *o = &e.objects[i];
        if (o->rail >= e.rail_count)
            die("%s: object %d uses rail %d of %d", galaxy_path, i, o->rail, e.rail_count);
        if (o->camera >= e.camera_count) die("%s: object %d uses camera %d", galaxy_path, i, o->camera);
    }
    return e;
}

static void bezier(const float p[4][3], float t, float out[3]);

static void wire_sample(const StageExtras *e, GravityDef *g, const char *path) {
    if (g->kind != GRAV_WIRE) return;
    if (g->rail < 0 || g->rail >= e->rail_count)
        die("%s: wire gravity without a valid rail (%d)", path, g->rail);
    const RailDef *d = &e->rails[g->rail];
    int parts = d->point_count < 2 ? 0 : d->closed ? d->point_count : d->point_count - 1;
    if (!parts) die("%s: wire gravity rail %d has no segment", path, g->rail);
    enum { STEPS = 1024 };
    int ns = parts * STEPS + 1;
    float (*pt)[3] = xmalloc(sizeof(float[3]) * (size_t)ns);
    float *acc = xmalloc(sizeof(float) * (size_t)ns);
    for (int i = 0; i < parts; i++) {
        const RailPointDef *a = &e->rail_points[d->first_point + i];
        const RailPointDef *b = &e->rail_points[d->first_point + (i + 1) % d->point_count];
        float cp[4][3];
        memcpy(cp[0], a->position, 12);
        memcpy(cp[1], a->out, 12);
        memcpy(cp[2], b->in, 12);
        memcpy(cp[3], b->position, 12);
        for (int s = i ? 1 : 0; s <= STEPS; s++) bezier(cp, (float)s / STEPS, pt[i * STEPS + s]);
    }
    acc[0] = 0;
    for (int s = 1; s < ns; s++) {
        float dd[3] = { pt[s][0] - pt[s - 1][0], pt[s][1] - pt[s - 1][1], pt[s][2] - pt[s - 1][2] };
        acc[s] = acc[s - 1] + len3(dd);
    }
    int count = g->wire_count - 1;
    float interval = acc[ns - 1] / (float)(g->wire_count - 1);
    g->wire = xmalloc(sizeof(float[3]) * (size_t)count);
    float lo[3] = { INFINITY, INFINITY, INFINITY }, hi[3] = { -INFINITY, -INFINITY, -INFINITY };
    for (int i = 0, s = 0; i < count; i++) {
        float want = interval * (float)i;
        while (s < ns - 1 && acc[s + 1] < want) s++;
        float seg = s < ns - 1 ? acc[s + 1] - acc[s] : 0, t = seg > 0 ? (want - acc[s]) / seg : 0;
        for (int k = 0; k < 3; k++) {
            float v = s < ns - 1 ? pt[s][k] + (pt[s + 1][k] - pt[s][k]) * t : pt[s][k];
            g->wire[i][k] = v;
            lo[k] = fminf(lo[k], v);
            hi[k] = fmaxf(hi[k], v);
        }
    }
    g->wire_count = count;
    for (int k = 0; k < 3; k++) {
        g->centre[k] = (lo[k] + hi[k]) * 0.5f;
        g->extent[k] = (hi[k] - lo[k]) * 0.5f;
    }
    free(pt);
    free(acc);
}

static int gravity_eval(const GravityDef *g, const float p[3], float dir[3], float *distance) {
    float q[3] = { p[0] - g->centre[0], p[1] - g->centre[1], p[2] - g->centre[2] };
    const float *ay = g->axis[1];
    int check_range = 1;
    switch (g->kind) {
    case GRAV_POINT: {
        float d = len3(q);
        if (d <= 0) return 0;
        for (int k = 0; k < 3; k++) dir[k] = -q[k] / d;
        *distance = d;
        break;
    }
    case GRAV_CUBE: {
        int region[3], outside = 0;
        float t[3];
        for (int k = 0; k < 3; k++) {
            t[k] = dot3(q, g->axis[k]);
            region[k] = t[k] < -g->extent[k] ? 0 : t[k] <= g->extent[k] ? 1 : 2;
            if (region[k] == 0 && !(g->faces & (2 << (2 * k)))) return 0;
            if (region[k] == 2 && !(g->faces & (1 << (2 * k)))) return 0;
            outside += region[k] != 1;
        }
        if (!outside) return 0;

        float target[3] = { g->centre[0], g->centre[1], g->centre[2] };
        for (int k = 0; k < 3; k++) {
            float c = region[k] == 0 ? -g->extent[k] : region[k] == 2 ? g->extent[k] : t[k];
            for (int i = 0; i < 3; i++) target[i] += g->axis[k][i] * c;
        }
        float w[3] = { target[0] - p[0], target[1] - p[1], target[2] - p[2] };
        float d = len3(w);
        if (d <= 0) return 0;
        for (int k = 0; k < 3; k++) dir[k] = w[k] / d;
        *distance = d;
        break;
    }
    case GRAV_PLANE:
        if (g->range >= 0 && len3(q) >= g->range) return 0;
        for (int k = 0; k < 3; k++) dir[k] = -ay[k];
        *distance = g->base;
        check_range = 0;
        break;
    case GRAV_PLANE_BOX:
        for (int k = 0; k < 3; k++)
            if (fabsf(dot3(q, g->axis[k])) > g->extent[k]) return 0;
        for (int k = 0; k < 3; k++) dir[k] = -ay[k];
        *distance = g->base + (g->faces < 3 ? fabsf(dot3(q, g->axis[g->faces])) : 0);
        check_range = 0;
        break;
    case GRAV_PLANE_CYLINDER: {
        float h = dot3(q, ay);
        if (h < 0 || h > g->extent[1]) return 0;
        float r[3] = { q[0] - ay[0] * h, q[1] - ay[1] * h, q[2] - ay[2] * h };
        float rl = len3(r);
        if (rl > g->extent[0]) return 0;
        for (int k = 0; k < 3; k++) dir[k] = -ay[k];
        *distance = g->base + rl;
        check_range = 0;
        break;
    }
    case GRAV_DISK: {
        float y = dot3(q, ay);
        if (!(g->flags & GRAVF_BOTH_SIDES) && y < 0) return 0;
        float r[3] = { q[0] - ay[0] * y, q[1] - ay[1] * y, q[2] - ay[2] * y };
        float rr = len3(r);
        if (rr > 0) normalize3(r);
        if (g->valid_cos > -1.0f && dot3(r, g->ref_side) < g->valid_cos) return 0;
        if (rr <= g->extent[0]) {
            for (int k = 0; k < 3; k++) dir[k] = y >= 0 ? -ay[k] : ay[k];
            *distance = fabsf(y);
        } else {
            if (!(g->flags & GRAVF_EDGE)) return 0;
            float w[3];
            for (int k = 0; k < 3; k++) w[k] = g->centre[k] + r[k] * g->extent[0] - p[k];
            float d = len3(w);
            if (d <= 0) return 0;
            for (int k = 0; k < 3; k++) dir[k] = w[k] / d;
            *distance = d;
        }
        break;
    }
    case GRAV_SEGMENT: {
        float tt = dot3(q, ay);
        if (g->valid_cos > -1.0f) {
            float r[3] = { q[0] - ay[0] * tt, q[1] - ay[1] * tt, q[2] - ay[2] * tt };
            normalize3(r);
            if (dot3(r, g->ref_side) < g->valid_cos) return 0;
        }
        if (tt < 0) {
            if (!(g->flags & GRAVF_EDGE0)) return 0;
            tt = 0;
        } else if (tt > g->extent[1]) {
            if (!(g->flags & GRAVF_EDGE1)) return 0;
            tt = g->extent[1];
        }
        float w[3];
        for (int k = 0; k < 3; k++) w[k] = g->centre[k] + ay[k] * tt - p[k];
        float d = len3(w);
        if (d <= 0) return 0;
        for (int k = 0; k < 3; k++) dir[k] = w[k] / d;
        *distance = d;
        break;
    }
    case GRAV_WIRE: {
        float best = INFINITY, target[3] = { 0, 0, 0 };
        for (int i = 0; i + 1 < g->wire_count; i++) {
            const float *a = g->wire[i], *b = g->wire[i + 1];
            float ab[3] = { b[0] - a[0], b[1] - a[1], b[2] - a[2] };
            float ap[3] = { p[0] - a[0], p[1] - a[1], p[2] - a[2] };
            float n = dot3(ab, ab), t = n > 0 ? fminf(fmaxf(dot3(ap, ab) / n, 0.0f), 1.0f) : 0.0f;
            float f[3] = { a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t };
            float w[3] = { f[0] - p[0], f[1] - p[1], f[2] - p[2] };
            float dd = dot3(w, w);
            if (dd < best) {
                best = dd;
                memcpy(target, f, 12);
            }
        }
        float w[3] = { target[0] - p[0], target[1] - p[1], target[2] - p[2] };
        float d = len3(w);
        if (!(best < INFINITY) || d <= 0) return 0;
        for (int k = 0; k < 3; k++) dir[k] = w[k] / d;
        *distance = d;
        break;
    }
    default: {
        float y = dot3(q, ay);
        if (!(g->flags & GRAVF_BOTH_SIDES) && y < 0) return 0;
        float r[3] = { q[0] - ay[0] * y, q[1] - ay[1] * y, q[2] - ay[2] * y };
        float rr = len3(r);
        if (rr > 0) normalize3(r);
        float inner = g->extent[0] - g->extent[1], target = -1;
        if (rr < inner) {
            if (g->faces == 0 || g->faces == 2) return 0;
            target = inner;
        } else if (rr > g->extent[0]) {
            if (g->faces == 0 || g->faces == 1) return 0;
            target = g->extent[0];
        }
        if (target < 0) {
            for (int k = 0; k < 3; k++) dir[k] = y >= 0 ? -ay[k] : ay[k];
            *distance = fabsf(y);
        } else {
            float w[3];
            for (int k = 0; k < 3; k++) w[k] = g->centre[k] + r[k] * target - p[k];
            float d = len3(w);
            if (d <= 0) return 0;
            for (int k = 0; k < 3; k++) dir[k] = w[k] / d;
            *distance = d;
        }
        break;
    }
    }
    if (check_range && g->range >= 0 && *distance >= g->range + g->distant) return 0;
    return 1;
}

void stage_extras_up_at(void *extras, const float p[3], float up[3]) {
    const StageExtras *e = extras;
    int best_priority = -100000, found = 0, legacy = 0;
    float sum[3] = { 0, 0, 0 }, nearest = INFINITY;
    up[0] = 0;
    up[1] = 0;
    up[2] = 1;
    for (int i = 0; i < e->gravity_count; i++) {
        const GravityDef *g = &e->gravities[i];
        if (found && g->priority < best_priority) break;
        if (g->switch_a >= 0 || g->switch_appear >= 0 || g->switch_sleep >= 0) continue;
        float dir[3], d;
        if (!gravity_eval(g, p, dir, &d)) continue;
        if (g->flags & GRAVF_INVERSE)
            for (int k = 0; k < 3; k++) dir[k] = -dir[k];
        found = 1;
        best_priority = g->priority;
        if (g->flags & GRAVF_LEGACY) {
            legacy = 1;
            if (d - g->distant < nearest) {
                nearest = d - g->distant;
                memcpy(sum, dir, 12);
            }
        } else if (!legacy) {
            float r = fmaxf(d - g->distant, 1.0f), w = 1.0f / (r * r);
            for (int k = 0; k < 3; k++) sum[k] += dir[k] * w;
        }
    }
    if (found && len3(sum) > 0) {
        normalize3(sum);
        for (int k = 0; k < 3; k++) up[k] = -sum[k];
    }
}

static int32_t fx(float v) { return float_to_fixed(v, 8); }
static int16_t unit14(float v) { return float_to_i16_saturated(roundf(v * 16384.0f)); }
static int16_t q12(float v) { return float_to_i16_saturated(roundf(v * 4096.0f)); }

Gravity gravity_to_asset(const GravityDef *g) {
    Gravity a = { 0 };
    a.kind = (uint8_t)g->kind;
    a.flags = (uint8_t)g->flags;
    a.faces = (uint8_t)g->faces;
    a.scenarios = (uint8_t)g->scenarios;
    a.priority = (int16_t)g->priority;
    a.switch_a = (int16_t)g->switch_a;
    a.switch_b = (int16_t)g->switch_b;
    a.switch_appear = (int16_t)g->switch_appear;
    a.switch_sleep = (int16_t)g->switch_sleep;
    a.valid_cos = g->valid_cos <= -1.0f ? -16384 : unit14(g->valid_cos);
    for (int k = 0; k < 3; k++) {
        a.centre[k] = fx(g->centre[k]);
        a.ref_side[k] = unit14(g->ref_side[k]);
        a.extent[k] = fx(g->extent[k]);
        for (int i = 0; i < 3; i++) a.axis[k][i] = unit14(g->axis[k][i]);
    }
    a.range = g->range < 0 ? -1 : fx(g->range);
    a.distant = fx(g->distant);
    a.base = fx(g->base);

    float reach = -1;
    float box = sqrtf(g->extent[0] * g->extent[0] + g->extent[1] * g->extent[1] +
                      g->extent[2] * g->extent[2]);
    switch (g->kind) {
    case GRAV_PLANE:
        reach = g->range;
        break;
    case GRAV_PLANE_BOX:
    case GRAV_PLANE_CYLINDER:
        reach = box;
        break;
    default:
        if (g->range >= 0) reach = box + g->range + g->distant;
        break;
    }
    a.bound = reach < 0 ? -1 : fx(reach) + 64;
    return a;
}

StageObject object_to_asset(const ObjectDef *o) {
    StageObject a = { 0 };
    a.type = (uint16_t)o->type;
    a.scenarios = (uint8_t)o->scenarios;
    a.flags = (uint8_t)o->flags;
    for (int k = 0; k < 3; k++) {
        a.position[k] = fx(o->position[k]);
        a.up[k] = unit14(o->up[k]);
        a.front[k] = unit14(o->front[k]);
        a.target[k] = fx(o->target[k]);
        a.scale[k] = float_to_i16_saturated(roundf(o->scale[k] * 256.0f));
    }
    for (int k = 0; k < 8; k++) a.args[k] = (int16_t)o->args[k];
    a.rail = (int16_t)o->rail;
    for (int k = 0; k < 5; k++) a.sw[k] = (int16_t)o->sw[k];
    a.camera = (int16_t)o->camera;
    a.id = (uint16_t)o->id;
    a.zone = (uint8_t)o->zone;
    return a;
}

StageStart start_to_asset(const StartDef *s) {
    StageStart a = { 0 };
    for (int k = 0; k < 3; k++) {
        a.position[k] = fx(s->position[k]);
        a.up[k] = unit14(s->up[k]);
        a.front[k] = unit14(s->front[k]);
    }
    a.id = (int16_t)s->id;
    a.scenarios = (uint8_t)s->scenarios;
    a.zone = (uint8_t)s->zone;
    a.camera = (int16_t)s->camera;
    return a;
}

StageArea area_to_asset(const AreaDef *d) {
    StageArea a = { 0 };
    a.cls = (uint8_t)d->cls;
    a.shape = (uint8_t)d->shape;
    a.scenarios = (uint8_t)d->scenarios;
    a.zone = (uint8_t)d->zone;
    for (int k = 0; k < 8; k++) a.args[k] = (int16_t)d->args[k];
    a.camera = (int16_t)d->camera;
    a.priority = (int16_t)d->priority;
    a.sw = (int16_t)d->sw;
    for (int k = 0; k < 3; k++) {
        a.centre[k] = fx(d->centre[k]);
        a.extent[k] = fx(d->extent[k]);
        for (int i = 0; i < 3; i++) a.axis[k][i] = unit14(d->axis[k][i]);
    }
    float reach = d->shape == SHAPE_CYLINDER
                      ? d->extent[0] + d->extent[1]
                      : sqrtf(d->extent[0] * d->extent[0] + d->extent[1] * d->extent[1] +
                              d->extent[2] * d->extent[2]);
    a.bound = fx(reach) + 64;
    return a;
}

StageCamera camera_to_asset(const CameraDef *c) {
    StageCamera a = { 0 };

    a.type = (uint8_t)(c->type == CAM_CHARMED_VECREG_TOWER && !c->reg_type ? CAM_XZ_PARA : c->type);
    a.vpan = (uint8_t)c->vpan;
    a.camint = (int16_t)c->camint;
    a.evfrm = (int16_t)c->evfrm;
    a.num1 = (int16_t)c->num1;
    a.num2 = (int16_t)(c->type == CAM_CHARMED_VECREG_TOWER ? c->reg_id : c->num2);
    a.pad = (uint8_t)c->reg_type;
    a.angle_a = q12(c->angle_a);
    a.angle_b = q12(c->angle_b);
    a.roll = q12(c->roll);
    a.upper = q12(c->upper);
    a.lower = q12(c->lower);
    a.fovy = (int16_t)lroundf(c->fovy);
    a.dist = fx(c->dist);

    int axis_dir = c->type == CAM_TOWER || c->type == CAM_TOWER_POS || c->type == CAM_FRONT_AND_BACK ||
                   c->type == CAM_TRUNDLE || c->type == CAM_CHARMED_VECREG_TOWER;
    int up_dir = c->type == CAM_POINT_FIX || c->type == CAM_CHARMED_FIX;
    for (int k = 0; k < 3; k++) {
        a.offset[k] = fx(c->offset[k]);
        a.wpoint[k] = fx(c->wpoint[k]);
        a.axis[k] = axis_dir ? unit14(c->axis[k]) : fx(c->axis[k]);
        a.up[k] = up_dir ? unit14(c->up[k]) : k == 0 ? fx(c->up[k]) : q12(c->up[k]);
        a.woffset[k] = fx(c->woffset[k]);
    }
    for (int k = 0; k < 3; k++) {
        a.zone_up[k] = unit14(c->zone_up[k]);
        a.extra[k] = unit14(c->extra[k]);
    }
    a.flags = (uint8_t)c->flags;
    a.camendint = c->camendint;
    a.loffset = fx(c->loffset);
    a.loffsetv = fx(c->loffsetv);
    a.uplay = fx(c->uplay);
    a.lplay = fx(c->lplay);
    return a;
}

static void bezier(const float p[4][3], float t, float out[3]) {
    float u = 1 - t;
    for (int k = 0; k < 3; k++)
        out[k] = u * u * u * p[0][k] + 3 * u * u * t * p[1][k] + 3 * u * t * t * p[2][k] +
                 t * t * t * p[3][k];
}

void rails_to_asset(const StageExtras *e, Vector *rails, Vector *points, Vector *parts) {
    *rails = VEC(StageRail);
    *points = VEC(StageRailPoint);
    *parts = VEC(StageRailPart);
    for (int r = 0; r < e->rail_count; r++) {
        const RailDef *d = &e->rails[r];
        StageRail *sr = vec_push(rails);
        memset(sr, 0, sizeof *sr);
        sr->first_point = (uint16_t)points->len;
        sr->point_count = (uint16_t)d->point_count;
        sr->first_part = (uint16_t)parts->len;
        sr->closed = (uint8_t)d->closed;
        sr->zone = (uint8_t)d->zone;
        for (int k = 0; k < 8; k++) sr->args[k] = (int16_t)d->args[k];
        for (int i = 0; i < d->point_count; i++) {
            const RailPointDef *pd = &e->rail_points[d->first_point + i];
            StageRailPoint *sp = vec_push(points);
            for (int k = 0; k < 3; k++) sp->position[k] = fx(pd->position[k]);
            for (int k = 0; k < 8; k++) sp->args[k] = (int16_t)pd->args[k];
        }
        int n_parts = d->point_count < 2 ? 0 : d->closed ? d->point_count : d->point_count - 1;
        float total = 0;
        for (int i = 0; i < n_parts; i++) {
            const RailPointDef *a = &e->rail_points[d->first_point + i];
            const RailPointDef *b = &e->rail_points[d->first_point + (i + 1) % d->point_count];
            float cp[4][3];
            memcpy(cp[0], a->position, 12);
            memcpy(cp[1], a->out, 12);
            memcpy(cp[2], b->in, 12);
            memcpy(cp[3], b->position, 12);

            float acc[17] = { 0 }, prev[3];
            bezier(cp, 0, prev);
            for (int s = 1; s <= 16 * 64; s++) {
                float q[3];
                bezier(cp, (float)s / (16 * 64), q);
                float dd[3] = { q[0] - prev[0], q[1] - prev[1], q[2] - prev[2] };
                acc[(s + 63) / 64] += len3(dd);
                memcpy(prev, q, 12);
            }
            for (int k = 1; k <= 16; k++) acc[k] += acc[k - 1];
            StageRailPart *sp = vec_push(parts);
            memset(sp, 0, sizeof *sp);
            for (int j = 0; j < 4; j++)
                for (int k = 0; k < 3; k++) sp->p[j][k] = fx(cp[j][k]);
            sp->start = fx(total);
            sp->length = fx(acc[16]);
            for (int k = 0; k < 16; k++)
                sp->lut[k] = (uint16_t)(acc[16] > 0 ? fminf(acc[k + 1] / acc[16] * 65535.0f + 0.5f, 65535.0f)
                                                     : 65535);
            total += acc[16];
        }
        sr->part_count = (uint16_t)n_parts;
        sr->length = fx(total);
    }
}

#define COL_LEAF_TRIS 6

typedef struct {
    const CollisionTriangle *tris;
    int axis;
} ColSort;

static int32_t col_centroid(const CollisionTriangle *t, int axis) {
    return (int32_t)t->v[0][axis] + t->v[1][axis] + t->v[2][axis];
}

static int compare_col(const void *x, const void *y, void *ctx) {
    const ColSort *s = ctx;
    int32_t a = col_centroid(&s->tris[*(const int *)x], s->axis);
    int32_t b = col_centroid(&s->tris[*(const int *)y], s->axis);
    if (a != b) return a < b ? -1 : 1;
    return (*(const int *)x > *(const int *)y) - (*(const int *)x < *(const int *)y);
}

static void col_node(const CollisionTriangle *tris, int *group, int n, Vector *order, Vector *nodes) {
    int lo[3] = { INT32_MAX, INT32_MAX, INT32_MAX }, hi[3] = { INT32_MIN, INT32_MIN, INT32_MIN };
    int clo[3] = { INT32_MAX, INT32_MAX, INT32_MAX }, chi[3] = { INT32_MIN, INT32_MIN, INT32_MIN };
    for (int g = 0; g < n; g++) {
        const CollisionTriangle *t = &tris[group[g]];
        for (int v = 0; v < 3; v++)
            for (int c = 0; c < 3; c++) {
                if (t->v[v][c] < lo[c]) lo[c] = t->v[v][c];
                if (t->v[v][c] > hi[c]) hi[c] = t->v[v][c];
            }
        for (int c = 0; c < 3; c++) {
            int x = col_centroid(t, c);
            if (x < clo[c]) clo[c] = x;
            if (x > chi[c]) chi[c] = x;
        }
    }
    size_t idx = nodes->len;
    CollisionNode *node = vec_push(nodes);
    memset(node, 0, sizeof *node);
    for (int c = 0; c < 3; c++) {
        node->min[c] = (int16_t)lo[c];
        node->max[c] = (int16_t)hi[c];
    }
    if (n <= COL_LEAF_TRIS) {
        node->next = (uint16_t)order->len;
        node->count = (uint8_t)n;
        for (int g = 0; g < n; g++) *(int *)vec_push(order) = group[g];
    } else {
        int ext[3] = { chi[0] - clo[0], chi[1] - clo[1], chi[2] - clo[2] };
        ColSort s = { tris, ext[0] >= ext[1] && ext[0] >= ext[2] ? 0 : ext[1] >= ext[2] ? 1 : 2 };
        stable_sort(group, (size_t)n, sizeof(int), compare_col, &s);
        col_node(tris, group, n / 2, order, nodes);
        col_node(tris, group + n / 2, n - n / 2, order, nodes);
    }
    CollisionNode *created = &VEC_AT(*nodes, CollisionNode, idx);
    if (!created->count) created->next = (uint16_t)nodes->len;
}

typedef struct { uint16_t p[3]; } ColCorners;

void collision_build(const Model *m, int shift, Vector *triangles, Vector *nodes) {
    Vector all = VEC(CollisionTriangle);
    Vector all_points = VEC(ColCorners);
    Vector all_part = VEC(uint8_t);
    for (size_t i = 0; i < m->triangles.len; i++) {
        const BuildTriangle *st = &VEC_AT(m->triangles, BuildTriangle, i);
        if (st->flags & SURF_UNSOLID) continue;
        *(uint8_t *)vec_push(&all_part) = st->pad;
        CollisionTriangle *t = vec_push(&all);
        memset(t, 0, sizeof *t);
        ColCorners *corners = vec_push(&all_points);
        for (int v = 0; v < 3; v++) corners->p[v] = st->v[v].point;
        uint16_t codes = VEC_AT(m->source_codes, uint16_t, i);
        t->floor_code = (uint8_t)codes;
        t->wall_code = (uint8_t)(codes >> 8);
        double p[3][3];
        for (int v = 0; v < 3; v++)
            for (int c = 0; c < 3; c++) {
                t->v[v][c] = VEC_AT(m->points, Point3, st->v[v].point)[c];
                p[v][c] = t->v[v][c] / 256.0;
            }
        double e1[3], e2[3], n[3];
        for (int c = 0; c < 3; c++) {
            e1[c] = p[1][c] - p[0][c];
            e2[c] = p[2][c] - p[0][c];
        }
        n[0] = e1[1] * e2[2] - e1[2] * e2[1];
        n[1] = e1[2] * e2[0] - e1[0] * e2[2];
        n[2] = e1[0] * e2[1] - e1[1] * e2[0];
        double len = sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2]);
        if (len < 1e-9) {
            all.len--;
            all_points.len--;
            all_part.len--;
            continue;
        }

        double s = (n[0] * st->normal[0] + n[1] * st->normal[1] + n[2] * st->normal[2]) >= 0 ? 1.0 : -1.0;
        double plane = 0;
        for (int c = 0; c < 3; c++) {
            t->normal[c] = (int16_t)lround(n[c] / len * s * 16384.0);
            plane += t->normal[c] * ((double)t->v[0][c] * (double)(1 << shift));
        }
        t->plane = (int32_t)llround(plane / 16384.0);
        uint16_t attr = st->flags & SURF_SCENARIO_MASK;
        if (st->flags & SURF_FLOOR) attr |= COL_FLOOR_ONLY;
        if (st->flags & SURF_DEATH) attr |= COL_DEATH;
        if (st->flags & SURF_SLIPPERY) attr |= COL_SLIPPERY;
        if (st->flags & SURF_HONEY) attr |= COL_HONEY;
        if (st->flags & SURF_NOCAMERA) attr |= COL_NO_CAMERA;
        if (st->flags & SURF_WATER) attr |= COL_WATER;
        if (st->flags & SURF_NOT_GRAB) attr |= COL_NOT_GRAB;
        if (st->ledges & TRI_SOURCE_ICE || t->floor_code == 5 || t->floor_code == 33) attr |= COL_ICE;
        if (st->ledges & TRI_SOURCE_BURN || t->floor_code == 10) attr |= COL_BURN;
        if (t->wall_code == 1) attr |= COL_NO_WALL_JUMP;
        if (t->wall_code == 3 || t->wall_code == 8) attr |= COL_NOT_GRAB;
        t->attr = attr;
    }
    if (all.len > 0xFFFF) die("too many collision triangles");
    int *group = xmalloc(sizeof(int) * (all.len + 1));
    Vector order = VEC(int);
    *nodes = VEC(CollisionNode);

    int max_part = 0;
    for (size_t i = 0; i < all_part.len; i++)
        if (VEC_AT(all_part, uint8_t, i) > max_part) max_part = VEC_AT(all_part, uint8_t, i);
    for (int part = 0; part <= max_part; part++) {
        int n = 0;
        for (size_t i = 0; i < all.len; i++)
            if (VEC_AT(all_part, uint8_t, i) == part) group[n++] = (int)i;
        if (!n) continue;
        size_t first_node = nodes->len;
        col_node(all.data, group, n, &order, nodes);
        if (part > 255) die("too many switched map parts");
        for (size_t k = first_node; k < nodes->len; k++)
            VEC_AT(*nodes, CollisionNode, k).part = (uint8_t)part;
    }
    if (nodes->len > 0xFFFF) die("too many collision nodes");

    *triangles = VEC(CollisionFace);
    for (size_t i = 0; i < order.len; i++) {
        int k = VEC_AT(order, int, i);
        const CollisionTriangle *t = &VEC_AT(all, CollisionTriangle, k);
        CollisionFace *f = vec_push(triangles);
        memset(f, 0, sizeof *f);
        memcpy(f->point, VEC_AT(all_points, ColCorners, k).p, sizeof f->point);
        for (int v = 0; v < 3; v++)
            for (int c = 0; c < 3; c++)
                if (VEC_AT(m->points, Point3, f->point[v])[c] != t->v[v][c])
                    die("collision corner differs from its stage point");
        memcpy(f->normal, t->normal, sizeof f->normal);
        f->attr = t->attr;
        f->floor_code = t->floor_code;
        f->wall_code = t->wall_code;
    }
    vec_free(&all_points);
    free(group);
    vec_free(&order);
    vec_free(&all);
    vec_free(&all_part);
}

static void rail_pos_at(const StageRail *r, const StageRailPart *parts, int32_t coord, float out[3]) {
    if (r->closed && r->length > 0) {
        coord %= r->length;
        if (coord < 0) coord += r->length;
    } else {
        coord = coord < 0 ? 0 : coord > r->length ? r->length : coord;
    }
    const StageRailPart *p = parts + r->first_part;
    int i = 0;
    while (i + 1 < r->part_count && coord > p[i].start + p[i].length) i++;
    p += i;
    int32_t along = coord - p->start;
    uint32_t share = p->length <= 0 || along <= 0 ? 0 : along >= p->length ? 65535u
                                                   : (uint32_t)(((int64_t)along << 16) / p->length);
    int k = 0;
    while (k < 15 && p->lut[k] < share) k++;
    uint32_t lo = k ? p->lut[k - 1] : 0, hi = p->lut[k];
    float frac = hi > lo ? (float)(share - lo) / (float)(hi - lo) : 0;
    float t = (k + fminf(frac, 1.0f)) / 16.0f, u = 1 - t;
    float b[4] = { u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t };
    for (int c = 0; c < 3; c++)
        out[c] = b[0] * p->p[0][c] + b[1] * p->p[1][c] + b[2] * p->p[2][c] + b[3] * p->p[3][c];
}

static void put_u16(Vector *v, int index, int value) {
    VEC_AT(*v, uint8_t, 2 * index) = (uint8_t)value;
    VEC_AT(*v, uint8_t, 2 * index + 1) = (uint8_t)(value >> 8);
}

int rock_drops_build(const StageExtras *e, const Model *m, int shift, const Vector *rails,
                     const Vector *parts, Vector *table) {
    *table = VEC(uint8_t);
    int n_rails = (int)rails->len;
    float *radius = calloc((size_t)n_rails + 1, sizeof(float));
    int any = 0;
    for (int i = 0; i < e->object_count; i++) {
        const ObjectDef *o = &e->objects[i];
        const char *name = object_type_name(o->type);
        if (strcmp(name, "rockcreator") && strcmp(name, "wanwanrolling")) continue;
        if (o->rail < 0 || o->rail >= n_rails) continue;
        float rad = 225.0f * (o->scale[0] > 0 ? o->scale[0] : 1.0f);
        if (rad > radius[o->rail]) radius[o->rail] = rad;
        any = 1;
    }
    if (!any) {
        free(radius);
        return 0;
    }

    size_t nt = m->triangles.len, ns = 0;
    float (*tri)[4][3] = malloc(nt * sizeof *tri);
    for (size_t i = 0; i < nt; i++) {
        const BuildTriangle *st = &VEC_AT(m->triangles, BuildTriangle, i);
        if (st->flags & SURF_UNSOLID) continue;
        float (*t)[3] = tri[ns];
        for (int v = 0; v < 3; v++)
            for (int c = 0; c < 3; c++)
                t[v][c] = (float)VEC_AT(m->points, Point3, st->v[v].point)[c] * (float)(1 << shift);
        float e1[3] = { t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2] };
        float e2[3] = { t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2] }, n[3];
        cross3(e1, e2, n);
        float len = len3(n);
        if (len < 1e-6f) continue;
        float s = (n[0] * st->normal[0] + n[1] * st->normal[1] + n[2] * st->normal[2]) >= 0 ? 1.0f : -1.0f;
        for (int c = 0; c < 3; c++) t[3][c] = n[c] / len * s;
        ns++;
    }
    Vector samples = VEC(uint8_t), entries = VEC(int);
    const StageRailPart *pp = (const StageRailPart *)parts->data;
    for (int r = 0; r < n_rails; r++) {
        const StageRail *sr = &VEC_AT(*rails, StageRail, r);
        if (radius[r] <= 0 || !sr->part_count) continue;
        int count = sr->length / ROCK_DROP_STEP + 2;
        float (*pos)[3] = malloc((size_t)count * sizeof *pos), lo[3], hi[3];
        for (int k = 0; k < count; k++) {
            rail_pos_at(sr, pp, k * ROCK_DROP_STEP, pos[k]);
            for (int c = 0; c < 3; c++) {
                if (!k || pos[k][c] < lo[c]) lo[c] = pos[k][c];
                if (!k || pos[k][c] > hi[c]) hi[c] = pos[k][c];
            }
        }
        const float reach = 1500.0f;
        size_t *near = malloc((ns + 1) * sizeof *near), nn = 0;
        for (size_t i = 0; i < ns; i++) {
            int out = 0;
            for (int c = 0; c < 3 && !out; c++) {
                float a = fminf(fminf(tri[i][0][c], tri[i][1][c]), tri[i][2][c]);
                float b = fmaxf(fmaxf(tri[i][0][c], tri[i][1][c]), tri[i][2][c]);
                out = b < lo[c] - reach || a > hi[c] + reach;
            }
            if (!out) near[nn++] = i;
        }
        int *drop = malloc((size_t)count * sizeof *drop);
        for (int k = 0; k < count; k++) {
            float g[3] = { pos[k][0] / 256.0f, pos[k][1] / 256.0f, pos[k][2] / 256.0f }, up[3];
            stage_extras_up_at((void *)e, g, up);
            float d[3] = { -up[0], -up[1], -up[2] }, best = INFINITY, best_t = 0;

            for (size_t j = 0; j < nn; j++) {
                float (*t)[3] = tri[near[j]];
                if (dot3(t[3], up) < 0.3f) continue;
                float e1[3] = { t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2] };
                float e2[3] = { t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2] }, pv[3], qv[3];
                cross3(d, e2, pv);
                float det = dot3(e1, pv);
                if (fabsf(det) < 1e-6f) continue;
                float sv[3] = { pos[k][0] - t[0][0], pos[k][1] - t[0][1], pos[k][2] - t[0][2] };
                float u = dot3(sv, pv) / det;
                if (u < 0 || u > 1) continue;
                cross3(sv, e1, qv);
                float v = dot3(d, qv) / det;
                if (v < 0 || u + v > 1) continue;
                float tt = dot3(e2, qv) / det;
                if (tt < -2 * radius[r] || tt > radius[r] + reach) continue;
                if (fabsf(tt - radius[r]) < best) {
                    best = fabsf(tt - radius[r]);
                    best_t = tt;
                }
            }

            drop[k] = best < INFINITY && best_t >= 0 ? (int)lroundf(best_t) : -1;
        }

        int found = 0, dmin = 32767, dmax = -32768;
        for (int k = 0; k < count; k++)
            if (drop[k] >= 0) {
                found++;
                dmin = drop[k] < dmin ? drop[k] : dmin;
                dmax = drop[k] > dmax ? drop[k] : dmax;
            }
        if (found && dmax > radius[r] + 600) found = 0;

        for (int k = 0; k < count; k++) {
            int v = -1;
            if (found && drop[k] >= 0) v = drop[k];
            else if (found) {
                for (int dist = 1; dist <= 4 && v < 0; dist++)
                    for (int side = -1; side <= 1; side += 2) {
                        int j = k + side * dist;
                        if (sr->closed) j = ((j % count) + count) % count;
                        if (j < 0 || j >= count || drop[j] < 0) continue;
                        v = (int)lroundf(drop[j] + (radius[r] - drop[j]) * dist / 5.0f);
                        break;
                    }
            }
            int q = v < 0 ? ROCK_DROP_NONE : (v + 2) / ROCK_DROP_UNIT;
            if (v >= 0 && q >= ROCK_DROP_NONE) die("rock rail %d: drop %d out of range", r, v);
            *(uint8_t *)vec_push(&samples) = (uint8_t)q;
        }
        free(drop);
        if (!found) samples.len -= (size_t)count;
        else {
            *(int *)vec_push(&entries) = r;
            *(int *)vec_push(&entries) = (int)(samples.len - (size_t)count);
        }
        printf("  rock rail %d (radius %.0f): %d samples, floor under %d, drop %d..%d%s\n", r, radius[r],
               count, found, found ? dmin : 0, found ? dmax : 0, found ? "" : " (keeps the rail)");
        free(near);
        free(pos);
    }
    free(tri);
    free(radius);
    int n = (int)entries.len / 2, head = 2 + 4 * n;
    if (!n) {
        vec_free(&samples);
        vec_free(&entries);
        return 0;
    }
    if (head + samples.len > 65535) die("rock drop table too large");
    for (int i = 0; i < head; i++) *(uint8_t *)vec_push(table) = 0;
    put_u16(table, 0, n);
    for (int i = 0; i < n; i++) {
        put_u16(table, 1 + 2 * i, VEC_AT(entries, int, 2 * i));
        put_u16(table, 2 + 2 * i, head + VEC_AT(entries, int, 2 * i + 1));
    }
    for (size_t i = 0; i < samples.len; i++) *(uint8_t *)vec_push(table) = VEC_AT(samples, uint8_t, i);
    vec_free(&samples);
    vec_free(&entries);
    return 1;
}
