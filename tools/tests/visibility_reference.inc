/* Scalar GNW visibility/transform reference. Axis rejection and cone tests
 * corrected to the GBA contract. Deliberately tests every plane at every
 * node, independently of production's inherited-plane-mask optimization. */
int reference_cull(CullJob* job){
    uint32_t out = 0;
    uint32_t i = 0;
    while(i < job->n){
        const StageChunk* chunk = &job->chunks[i];
        int64_t center[3], extent[3];
        for(int axis = 0; axis < 3; axis++){
            center[axis] = (int32_t)chunk->min[axis] + chunk->max[axis];
            extent[axis] = (int32_t)chunk->max[axis] - chunk->min[axis];
        }

        bool visible = true;
        for(int plane = 0; plane < 5; plane++){
            const int32_t* p = job->planes[plane];
            int64_t midpoint = 0, radius = 0;
            for(int axis = 0; axis < 3; axis++){
                midpoint += center[axis] * p[axis];
                radius += extent[axis] * p[axis + 3];
            }
            /* The assembly halves center/extents, then shifts the result by 8. */
            int64_t center = midpoint >> 1;
            int64_t spread = radius >> 1;
            int64_t low = (center - spread) >> 8;
            int64_t high = (center + spread) >> 8;
            low += p[6];
            high += p[6];
            if(plane < 4){
                if(low > 64){
                    visible = false;
                    break;
                }
            }else if(low - 64 >= job->z_drop ||
                     (job->z_inner != 0 && high < job->z_inner)){
                visible = false;
                break;
            }
        }

        if(chunk->cone_plane_limit != CONE_NONE){
            int64_t delta[3];
            for(int k = 0; k < 3; k++) delta[k] = (int64_t)job->cam[k] - chunk->cone_center[k];
            if(llabs(delta[0]) < 131072 && llabs(delta[1]) < 131072 && llabs(delta[2]) < 131072){
                int64_t value = chunk->cone_plane_limit;
                for(int k = 0; k < 3; k++)
                    value += 2 * (delta[k] * chunk->cone_normal[k] +
                                  llabs(delta[k]) * chunk->cone_normal_extent[k]);
                if(value <= -768) visible = false;
            }
        }
        if(!visible){
            i = chunk->skip;
            continue;
        }
        if(chunk->leaf != UINT16_MAX)
            job->leaves[out++] = chunk->leaf;
        i++;
    }
    job->leaf_count = out;
    return (int)out;
}
static void reference_collect(CollectJob* job, bool indexed){
    const int axis = job->axis_coord / 2;
    for(uint32_t li = 0; li < (uint32_t)job->leaf_count; li++){
        uint16_t leaf = job->leaves[li];
        uint32_t first = job->leaf_triangle_offsets[leaf];
        uint32_t end = job->leaf_triangle_offsets[leaf + 1];
        for(uint32_t i = first; i < end; i++){
            if(job->plist >= job->plist_end) return;

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

            int32_t facing = 0;
            for(unsigned k = 0; k < 3; k++){
                int32_t delta = anchor[k] - job->cam[k];
                facing += (int32_t)(((int64_t)delta * normal[k]) >> 8);
            }
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
            if(reject) continue;

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
}

void reference_transform(const TransformJob* job){
    for(int i = 0; i < job->n; i++){
        uint16_t point = job->list ? job->list[i] : (uint16_t)i;
        uint32_t slot = point;
        if(job->home){
            uint16_t leaf = job->home[point];
            slot = (uint32_t)((int32_t)job->delta[leaf] + point);
        }

        const int16_t* src = job->src[point];
        FramePoint* dst = &job->dst[slot];
        int32_t* out = (int32_t*)&dst->position;
        for(unsigned row = 0; row < 3; row++){
            const int32_t* m = job->m[row];
            int32_t dot = m[0] * src[0] + m[1] * src[1] + m[2] * src[2];
            out[row] = m[3] + (dot >> 8);
        }

        int32_t x = dst->position.x, y = dst->position.y, z = dst->position.z;
        uint8_t clip = 0;
        if(-x > z) clip |= 2;
        if(x > z) clip |= 1;
        if(-y > z) clip |= 4;
        if(y > z) clip |= 8;
        int32_t guard = z << 2;
        if(z < 160 || x > guard || -x > guard || y > guard || -y > guard)
            clip |= 16;
        dst->clip = clip;
        if(!(clip & 16))
            job->screen[slot] = screen_pack_position(project_to_screen(dst->position));
    }
}
