#ifndef SMG_COMPACT_POINTS_H
#define SMG_COMPACT_POINTS_H

/* Claim-time positions hold source XYZ. Transformation overwrites them only
 * after this entire job has finished. UVs and face attributes stay per face. */
typedef struct {
    const uint16_t* visible;
    uint32_t count;
    const StageTriangle* triangles;
    const int16_t (*source)[3];
    uint16_t* slots;
    FramePoint* pool;
    uint16_t* list;
    uint16_t* buckets;
    uint32_t bucket_count, capacity, used, admitted;
    uint32_t map_hits, position_hits;
    uint8_t stamp;
} CompactPointJob;

static inline bool compact_same_position(const FramePoint* point, const int16_t* xyz){
    return point->position.x == xyz[0] && point->position.y == xyz[1] &&
           point->position.z == xyz[2];
}

static inline uint32_t compact_position_hash(const int16_t* xyz, uint32_t mask){
    uint32_t hash = (uint16_t)xyz[0] * UINT32_C(0x8da6b343);
    hash ^= (uint16_t)xyz[1] * UINT32_C(0xd8163841);
    hash ^= (uint16_t)xyz[2] * UINT32_C(0xcb1ab31f);
    return (hash ^ (hash >> 16)) & mask;
}

static inline uint16_t compact_find_point(CompactPointJob* job, uint16_t point){
    const int16_t* xyz = job->source[point];
    uint16_t slot = job->slots[point];
    /* used starts at zero each frame: every lower slot has already been
     * rewritten this frame. This also rejects stale mappings at stamp wrap,
     * stage changes and after the shared pool was used for collision scratch. */
    if(slot < job->used && job->pool[slot].mark == job->stamp &&
       compact_same_position(&job->pool[slot], xyz)){
        job->map_hits++;
        return slot;
    }
    uint32_t bucket = compact_position_hash(xyz, job->bucket_count - 1);
    for(slot = job->buckets[bucket]; slot != UINT16_MAX; slot = job->pool[slot].pad){
        if(compact_same_position(&job->pool[slot], xyz)){
            job->position_hits++;
            return slot;
        }
    }
    return UINT16_MAX;
}

static void geometry_claim_compact_points(CompactPointJob* job){
    job->used = job->admitted = job->map_hits = job->position_hits = 0;
    for(uint32_t i = 0; i < job->bucket_count; i++) job->buckets[i] = UINT16_MAX;
    for(uint32_t face = 0; face < job->count; face++){
        const StageTriangle* triangle = &job->triangles[job->visible[face]];
        if(job->capacity - job->used < 3){
            /* Preflight the last few slots atomically, including duplicates
             * inside this face. Capacity failure must leave its bindings and
             * the hash unchanged and retain the original ordered prefix. */
            const int16_t* missing[3];
            unsigned needed = 0;
            for(unsigned k = 0; k < 3; k++){
                uint16_t point = triangle->v[k].point;
                if(compact_find_point(job, point) != UINT16_MAX) continue;
                const int16_t* xyz = job->source[point];
                bool repeated = false;
                for(unsigned j = 0; j < needed; j++)
                    if(xyz[0] == missing[j][0] && xyz[1] == missing[j][1] &&
                       xyz[2] == missing[j][2]) repeated = true;
                if(!repeated) missing[needed++] = xyz;
            }
            if(needed > job->capacity - job->used) break;
        }
        for(unsigned k = 0; k < 3; k++){
            uint16_t point = triangle->v[k].point;
            uint16_t slot = compact_find_point(job, point);
            if(slot == UINT16_MAX){
                const int16_t* xyz = job->source[point];
                uint32_t bucket = compact_position_hash(xyz, job->bucket_count - 1);
                slot = (uint16_t)job->used++;
                FramePoint* destination = &job->pool[slot];
                destination->position = (v3){xyz[0], xyz[1], xyz[2]};
                destination->mark = job->stamp;
                destination->pad = job->buckets[bucket];
                job->buckets[bucket] = slot;
                job->list[slot] = point;
            }
            job->slots[point] = slot;
        }
        job->admitted++;
    }
}
#endif
