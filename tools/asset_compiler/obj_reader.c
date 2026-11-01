#include "obj_reader.h"

#include <ctype.h>
#include <stdio.h>
#include <string.h>

#ifdef SMG_ASSETC_PROGRESS
extern void smg_assetc_progress(uint32_t phase, uint32_t index, uint32_t total);
#define OBJ_PROGRESS(index, total) smg_assetc_progress(33, (index), (total))
#else
#define OBJ_PROGRESS(index, total) ((void)0)
#endif

static int split_terms(char *line, char **terms, int max) {
    int n = 0;
    char *p = line;
    while (*p && n < max) {
        while (*p && isspace((unsigned char)*p)) p++;
        if (!*p) break;
        terms[n++] = p;
        while (*p && !isspace((unsigned char)*p)) p++;
        if (*p) *p++ = 0;
    }
    return n;
}

static int parse_float(const char *s, float *out) {
    char *end;
    float v = strtof(s, &end);
    if (end == s || *end) return 0;
    *out = v;
    return 1;
}

static void parse_floats3(char **terms, int n, float *out) {
    out[0] = out[1] = out[2] = 0.0f;
    for (int i = 0; i < n && i < 3; i++)
        if (!parse_float(terms[i], &out[i])) break;
}

static int name_is_valid(const char *s) {
    for (; *s; s++)
        if (!isalnum((unsigned char)*s) && *s != '.' && *s != '_' && *s != '-') return 0;
    return 1;
}

static ObjGroup *find_group(Vector *groups, const char *name) {
    for (size_t i = 0; i < groups->len; i++)
        if (!strcmp(VEC_AT(*groups, ObjGroup, i).name, name)) return &VEC_AT(*groups, ObjGroup, i);
    ObjGroup *g = vec_push(groups);
    g->name = xstrdup(name);
    g->faces = VEC(ObjFace);
    return g;
}

static void flush_object(Obj *obj, const char *name, Vector *groups, Vector *default_group) {
    if (default_group->len) {
        ObjGroup *g = vec_push(groups);
        g->name = xstrdup("");
        g->faces = *default_group;
        *default_group = VEC(ObjFace);
    }
    if (!groups->len) return;
    ObjObject *o = NULL;
    for (size_t i = 0; i < obj->objects.len; i++)
        if (!strcmp(VEC_AT(obj->objects, ObjObject, i).name, name)) o = &VEC_AT(obj->objects, ObjObject, i);
    if (!o) {
        o = vec_push(&obj->objects);
        o->name = xstrdup(name);
    }
    o->groups = *groups;
    *groups = VEC(ObjGroup);
}

static int compare_object_names(const void *a, const void *b, void *ctx) {
    (void)ctx;
    return strcmp(((const ObjObject *)a)->name, ((const ObjObject *)b)->name);
}

static int compare_group_names(const void *a, const void *b, void *ctx) {
    (void)ctx;
    return strcmp(((const ObjGroup *)a)->name, ((const ObjGroup *)b)->name);
}

Obj obj_load(const char *path) {
    OBJ_PROGRESS(0, 0);
    char *text = (char *)read_file(path, NULL);
    if (!text) die("cannot open %s", path);
    OBJ_PROGRESS(1, 0);

    Obj obj = { VEC(float[3]), VEC(float[3]), VEC(ObjVertex), VEC(char *), VEC(ObjObject) };
    Vector groups = VEC(ObjGroup), default_group = VEC(ObjFace);
    Vector selected = VEC(char *);
    char *object_name = xstrdup("");

    int line_num = 0;
    for (char *line = text; line && *line;) {
        char *next = strchr(line, '\n');
        if (next) *next++ = 0;
        line_num++;
        if (line_num % 10000 == 0) OBJ_PROGRESS((uint32_t)line_num, 0);

        char *terms[64];
        int n = split_terms(line, terms, 64);
        if (n == 0) {
            line = next;
            continue;
        }

        if (!strcmp(terms[0], "v")) {
            parse_floats3(terms + 1, n - 1, vec_push(&obj.positions));
        } else if (!strcmp(terms[0], "vt")) {
            parse_floats3(terms + 1, n - 1, vec_push(&obj.uvs));
        } else if (!strcmp(terms[0], "f")) {
            ObjFace face = { (int)obj.verts.len, 0, (int)obj.materials.len - 1 };
            for (int t = 1; t < n; t++) {
                ObjVertex *v = vec_push(&obj.verts);
                int idx[2] = { 0, 0 }, count[2] = { (int)obj.positions.len, (int)obj.uvs.len };
                char *p = terms[t];
                for (int k = 0; k < 2; k++) {
                    char *slash = strchr(p, '/');
                    if (slash) *slash = 0;
                    if (*p) {
                        long i = strtol(p, NULL, 10);
                        if (i == 0) die("%s:%d: invalid index", path, line_num);
                        idx[k] = i > 0 ? (int)i : count[k] + (int)i + 1;
                    } else if (k == 0) {
                        die("%s:%d: missing position index", path, line_num);
                    }
                    if (!slash) break;
                    p = slash + 1;
                }
                v->pos = idx[0] - 1;
                v->uv = idx[1] - 1;
                face.count++;
            }
            if (selected.len == 0) {
                *(ObjFace *)vec_push(&default_group) = face;
            } else {
                for (size_t i = 0; i < selected.len; i++)
                    *(ObjFace *)vec_push(&find_group(&groups, VEC_AT(selected, char *, i))->faces) = face;
            }
        } else if (!strcmp(terms[0], "g")) {
            selected.len = 0;
            for (int t = 1; t < n; t++) {
                if (!name_is_valid(terms[t])) continue;
                find_group(&groups, terms[t]);
                *(char **)vec_push(&selected) = xstrdup(terms[t]);
            }
        } else if (!strcmp(terms[0], "o")) {
            flush_object(&obj, object_name, &groups, &default_group);
            selected.len = 0;
            if (n < 2 || !name_is_valid(terms[1])) die("%s:%d: bad object name", path, line_num);
            object_name = xstrdup(terms[1]);
        } else if (!strcmp(terms[0], "usemtl")) {
            if (n < 2) die("%s:%d: usemtl without a name", path, line_num);
            *(char **)vec_push(&obj.materials) = xstrdup(terms[1]);
        }
        line = next;
    }
    flush_object(&obj, object_name, &groups, &default_group);

    for (size_t i = 0; i < obj.verts.len; i++) {
        ObjVertex *v = &VEC_AT(obj.verts, ObjVertex, i);
        if (v->pos < 0 || v->pos >= (int)obj.positions.len) die("%s: position index out of range", path);
        if (v->uv >= (int)obj.uvs.len) die("%s: uv index out of range", path);
    }

    stable_sort(obj.objects.data, obj.objects.len, sizeof(ObjObject), compare_object_names, NULL);
    for (size_t i = 0; i < obj.objects.len; i++) {
        Vector *g = &VEC_AT(obj.objects, ObjObject, i).groups;
        stable_sort(g->data, g->len, sizeof(ObjGroup), compare_group_names, NULL);
    }
    free(text);
    OBJ_PROGRESS((uint32_t)line_num + 1, 0);
    return obj;
}

Mtl mtl_load(const char *path) {
    char *text = (char *)read_file(path, NULL);
    if (!text) die("cannot open %s", path);
    Mtl mtl = { 0 };
    const char *current = NULL;
    for (char *line = text; line && *line;) {
        char *next = strchr(line, '\n');
        if (next) *next++ = 0;
        char *terms[8];
        int n = split_terms(line, terms, 8);
        if (n >= 2 && !strcmp(terms[0], "newmtl")) {
            current = xstrdup(terms[1]);
        } else if (n >= 2 && current && (!strcmp(terms[0], "map_Kd") || !strcmp(terms[0], "map_d"))) {
            if (!mtl_texture(&mtl, current)) {
                char *texture = xstrdup(terms[1]);
                for (char *p = texture; *p; p++)
                    if (*p == '\\') *p = '/';
                while (*texture == '/') texture++;
                mtl.names = xrealloc(mtl.names, sizeof(char *) * (mtl.n + 1));
                mtl.textures = xrealloc(mtl.textures, sizeof(char *) * (mtl.n + 1));
                mtl.names[mtl.n] = xstrdup(current);
                mtl.textures[mtl.n] = texture;
                mtl.n++;
            }
        }
        line = next;
    }
    free(text);
    return mtl;
}

const char *mtl_texture(const Mtl *mtl, const char *material) {
    for (int i = 0; i < mtl->n; i++)
        if (!strcmp(mtl->names[i], material)) return mtl->textures[i];
    return NULL;
}
