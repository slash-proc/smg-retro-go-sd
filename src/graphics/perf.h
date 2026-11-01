#ifndef SMG_PERF_H
#define SMG_PERF_H

#include <stdint.h>

enum {
    SMG_PERF_LOGIC, SMG_PERF_WORLD, SMG_PERF_VISIBILITY, SMG_PERF_TRANSFORM,
    SMG_PERF_EMIT, SMG_PERF_RASTER, SMG_PERF_PALETTE, SMG_PERF_SCALE,
    SMG_PERF_HUD, SMG_PERF_STAGE_COUNT
};

/* Independent debug ABI: cumulative event counts and high-water marks.
 * Read at a frame/completed-call boundary for coherent totals. Collection
 * tested/rejected counts publish on return; counters wrap modulo 2^32. */
typedef struct {
    uint32_t version, rendered_frames, simulation_ticks;
    uint32_t chunks_visited, planes_tested, chunks_frustum_rejected, chunks_cone_rejected;
    uint32_t leaf_triangles_tested, leaf_triangles_rejected;
    uint32_t visible_capacity_hits, point_list_capacity_hits, point_pool_capacity_hits;
    uint32_t arena_attempts, arena_dropped, arena_peak, point_pool_peak, visible_peak;
    uint32_t far_capacity_skips, emitted_fast, emitted_slow, subdivisions;
    uint32_t raster_triangles, mip0_triangles, mip1_triangles, flat_triangles;
    uint32_t depth_effect_dropped, depth_effect_peak;
} SmgGeometryStats;

/* Optional SMG_RASTER_WORK_STATS diagnostic build only. Counts describe
 * submitted fragments, including overdraw, rather than unique LCD pixels.
 * Read after completed render calls; all fields wrap modulo 2^32. Diagnostic
 * instruction/timing overhead must not be used as an optimization benchmark. */
typedef struct {
    uint32_t triangles, covered_pixels, depth_rejected_pixels, texture_samples;
    uint32_t zero_texels, colour_writes, depth_writes;
    uint32_t perspective_spans, perspective_pixels, perspective_divisions;
} SmgRasterModeWork;
typedef struct {
    uint32_t version;
    SmgRasterModeWork mode[7];
} SmgRasterWorkStats;

#if defined(__GNW__)
extern SmgGeometryStats smg_geometry_stats;
#define SMG_COUNT(field) (++smg_geometry_stats.field)
#define SMG_ADD(field, count) (smg_geometry_stats.field += (uint32_t)(count))
#define SMG_PEAK(field, value) do { \
    uint32_t peak_value = (uint32_t)(value); \
    if(peak_value > smg_geometry_stats.field) smg_geometry_stats.field = peak_value; \
} while(0)
/* DWT is already enabled by the firmware. Never reset its shared counter.
 * GWemu may return zero; wall-clock measurements remain available there. */
static inline uint32_t smg_perf_now(void){
    return *(volatile uint32_t*)0xE0001004;
}
extern uint32_t smg_perf_frame_cycles[SMG_PERF_STAGE_COUNT];
static inline void smg_perf_add(unsigned stage, uint32_t start){
    smg_perf_frame_cycles[stage] += smg_perf_now() - start;
}
#else
#define SMG_COUNT(field) ((void)0)
#define SMG_ADD(field, count) ((void)(count))
#define SMG_PEAK(field, value) ((void)0)
static inline uint32_t smg_perf_now(void){ return 0; }
static inline void smg_perf_add(unsigned stage, uint32_t start){ (void)stage; (void)start; }
#endif

#endif
