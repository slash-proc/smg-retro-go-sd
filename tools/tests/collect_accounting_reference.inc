/* Frozen AO immediate counter updates: accounting oracle only.
 * Geometry/math correctness is checked by separate independent fixtures. */
static volatile bool reference_g_collect_narrow_enabled = true;
struct {
    uint32_t version, narrow_jobs, wide_jobs;
} reference_smg_collect_math_stats = {.version = 1};

static bool reference_collect_narrow_camera(const CollectJob* job){
    for(unsigned k = 0; k < 3; k++)
        if(job->cam[k] < INT16_MIN || job->cam[k] > INT16_MAX) return false;
    return true;
}

static inline int32_t reference_collect_facing_narrow(const int32_t anchor[3],
        const int32_t normal[3], const int32_t camera[3]){
    /* Anchor and camera are signed16; normals are signed16 (or doubled
     * signed8 for indexed records). |delta| <= 65535, so each product fits
     * signed32, including normal -32768. Shift each product separately:
     * shifting a combined dot product changes rounding/rejection. The sum
     * of three shifted products also fits signed32. */
    int32_t facing = 0;
    for(unsigned k = 0; k < 3; k++)
        facing += ((anchor[k] - camera[k]) * normal[k]) >> 8;
    return facing;
}

/* End collect facing helpers. */

__attribute__((always_inline)) static inline void reference_geometry_collect_leaves_mode(
        CollectJob* job, bool indexed, bool narrow){
    CollectJob* output = job;
    CollectJob parameters = *job;
    job = &parameters;
    const int axis = job->axis_coord / 2;
    for(uint32_t li = 0; li < (uint32_t)job->leaf_count; li++){
        uint16_t leaf = job->leaves[li];
        uint32_t first = job->leaf_triangle_offsets[leaf];
        uint32_t end = job->leaf_triangle_offsets[leaf + 1];
        for(uint32_t i = first; i < end; i++){
            if(job->plist >= job->plist_end){
                SMG_COUNT(point_list_capacity_hits);
                goto complete;
            }
            SMG_COUNT(leaf_triangles_tested);

            uint16_t points[3];
            int32_t anchor[3], normal[3];
            if(indexed){
                const StageTriangle* triangle = &job->triangles[i];
                for(unsigned k = 0; k < 3; k++)
                    points[k] = triangle->v[k].point;
                for(unsigned k = 0; k < 3; k++){
                    anchor[k] = job->points[points[0]][k];
                    normal[k] = (int32_t)triangle->normal[k] * 2;
                }
            }else{
                const LeafTriangle* record = &job->recs[i];
                for(unsigned k = 0; k < 3; k++){
                    points[k] = record->points[k];
                    anchor[k] = record->a[k];
                    normal[k] = record->normal[k];
                }
            }

            int32_t facing;
            if(narrow) facing = reference_collect_facing_narrow(anchor, normal, job->cam);
            else {
                facing = 0;
                for(unsigned k = 0; k < 3; k++){
                    int32_t delta = anchor[k] - job->cam[k];
                    facing += (int32_t)(((int64_t)delta * normal[k]) >> 8);
                }
            }
            /* Original COLLECT_INDEXED/geometry_collect_leaves branches to
             * the next triangle on nonnegative facing, before axis testing. */
            bool reject = facing >= 0;
            if(facing < 0){
                int32_t farthest = anchor[axis];
                if(job->axis_sign) farthest = -farthest;
                if(farthest > job->limit){
                    for(unsigned k = 1; k < 3; k++){
                        int32_t coordinate = job->points[points[k]][axis];
                        if(job->axis_sign) coordinate = -coordinate;
                        if(farthest > coordinate) farthest = coordinate;
                    }
                    reject = farthest != 32768 && farthest > job->limit;
                }
            }
            if(reject){
                SMG_COUNT(leaf_triangles_rejected);
                continue;
            }

            if(job->home == NULL){
                for(unsigned k = 0; k < 3; k++){
                    uint16_t point = points[k];
                    if(job->marks[point] != (uint8_t)job->stamp){
                        job->marks[point] = (uint8_t)job->stamp;
                        *job->plist++ = point;
                    }
                }
            }

            uint16_t rank = job->ranks[i];
            uint32_t word = rank >> 5;
            job->bits[word] |= UINT32_C(1) << (rank & 31);
            if(word < job->lo) job->lo = word;
            if(word > job->hi) job->hi = word;
        }
    }
complete:
    output->plist = job->plist;
    output->lo = job->lo;
    output->hi = job->hi;
}

static void reference_geometry_collect_leaves_c(CollectJob* job, bool indexed){
    /* Specialize before the triangle loop so narrow jobs do not carry wide
     * product temporaries or a per-triangle arithmetic-mode branch. */
    if(reference_g_collect_narrow_enabled && reference_collect_narrow_camera(job)){
        reference_smg_collect_math_stats.narrow_jobs++;
        reference_geometry_collect_leaves_mode(job, indexed, true);
    }else{
        reference_smg_collect_math_stats.wide_jobs++;
        reference_geometry_collect_leaves_mode(job, indexed, false);
    }
}

