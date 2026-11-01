/* Validated Y prototype, frozen only to check specialization pixel/depth equivalence.
 * Independent rational/GBA fixtures remain the correctness oracles. */
static int64_t y_raster_edge(int32_t ax, int32_t ay, int32_t bx, int32_t by, int32_t px,
                           int32_t py){
    return (int64_t)(bx - ax) * (py - ay) - (int64_t)(by - ay) * (px - ax);
}

static uint64_t y_raster_abs64(int64_t value){
    return (uint64_t)(value < 0 ? -value : value);
}

static volatile bool y_g_raster_setup_recip_enabled = true;

/* Setup numerators are below 2^63 (signed-16 screen/UV inputs, scaled by
 * at most 2^20). For R=floor(2^63/d), floor(n*R/2^63) underestimates n/d by
 * at most one. A remainder check recovers the exact truncating quotient.
 * Compute the 128-bit product with four 32x32 multiplies for Cortex-M7. */
__attribute__((always_inline)) static inline int64_t y_raster_setup_div(
    int64_t numerator, int64_t denominator, uint64_t* reciprocal, bool enabled){
    if(!numerator) return 0;
    if(!enabled) return numerator / denominator;
    uint64_t n = y_raster_abs64(numerator), d = y_raster_abs64(denominator);
    if(!*reciprocal) *reciprocal = (UINT64_C(1) << 63) / d;
    uint64_t a0 = (uint32_t)n, a1 = n >> 32;
    uint64_t b0 = (uint32_t)*reciprocal, b1 = *reciprocal >> 32;
    uint64_t p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
    uint64_t middle = (p00 >> 32) + (uint32_t)p01 + (uint32_t)p10;
    uint64_t high = p11 + (p01 >> 32) + (p10 >> 32) + (middle >> 32);
    uint64_t quotient = (high << 1) | ((middle >> 31) & 1);
    if(n - quotient * d >= d) quotient++;
    return (numerator < 0) != (denominator < 0) ? -(int64_t)quotient : (int64_t)quotient;
}

/* Perspective attributes use unnormalized barycentric numerators. Keeping
 * their common screen-area factor avoids setup divisions. For a rational UV
 * over a span, chord error is bounded by |du|*|dq|/(4*min(q0,q1)). Require
 * <=1 model UV unit (1/16 source texel). One/two-pixel spans are exact at
 * sampled endpoints. Use double only per span; the pixel loop remains Q16.
 * Signed-16 UVs and positive depths bound coordinates/steps to +/-65536. */
typedef struct {
    unsigned count;
    uint32_t u, v, du, dv;
} y_PerspectiveSpan;

/* Split whole/fractional model units so Cortex-M7 uses native F64->S32
 * conversions instead of a double-to-int64 helper call. Unsigned arithmetic
 * retains Q16 wrapping, including negative UVs and two-pixel long steps. */
static uint32_t y_raster_uv_q16(double value){
    int32_t whole = (int32_t)value;
    int32_t fraction = (int32_t)((value - whole) * 65536);
    return ((uint32_t)whole << 16) + (uint32_t)fraction;
}

static y_PerspectiveSpan y_raster_perspective_span(double q, double uq, double vq,
        double dq, double duq, double dvq, unsigned remaining){
    unsigned count = remaining < 8 ? remaining : 8;
    double inverse = 1 / q;
    double u0 = uq * inverse, v0 = vq * inverse, u1 = u0, v1 = v0;
    while(count > 1){
        double last = count - 1;
        double q1 = q + dq * last;
        inverse = 1 / q1;
        u1 = (uq + duq * last) * inverse;
        v1 = (vq + dvq * last) * inverse;
        double change = q1 > q ? q1 - q : q - q1;
        double minimum = q1 < q ? q1 : q;
        double eu = (u1 > u0 ? u1 - u0 : u0 - u1) * change;
        double ev = (v1 > v0 ? v1 - v0 : v0 - v1) * change;
        if(count <= 2 || (eu <= 4 * minimum && ev <= 4 * minimum)) break;
        count = (count + 1) / 2;
    }
    y_PerspectiveSpan span = {count, y_raster_uv_q16(u0), y_raster_uv_q16(v0), 0, 0};
    if(count > 1){
        static const double inverse_steps[8] = {0, 1, .5, 1.0/3, .25, .2, 1.0/6, 1.0/7};
        double step = inverse_steps[count - 1];
        span.du = y_raster_uv_q16((u1 - u0) * step);
        span.dv = y_raster_uv_q16((v1 - v0) * step);
    }
    return span;
}

static bool y_raster_span_bound(int64_t weight, int64_t step, int* first, int* last){
    /* Solve weight + x*step >= 0 once per row instead of at every pixel.
     * Packed screen coordinates are int16; at <=320 pixels the divisors and
     * relevant distances fit uint32, using the M7 hardware divider. Keep
     * 64-bit setup so extreme offscreen triangles retain exact coverage. */
    if(step > 0 && weight < 0){
        uint64_t distance = (uint64_t)-weight;
        if(distance > (uint64_t)*last * step) return false;
        unsigned at = ((uint32_t)distance + (uint32_t)step - 1) / (uint32_t)step;
        if((int)at > *first) *first = at;
    } else if(step < 0){
        if(weight < 0) return false;
        uint32_t divisor = (uint32_t)-step;
        if((uint64_t)weight < (uint64_t)*last * divisor){
            unsigned at = (uint32_t)weight / divisor;
            if((int)at < *last) *last = at;
        }
    } else if(step == 0 && weight < 0) return false;
    return *first <= *last;
}

__attribute__((noinline)) static void y_engine_raster_inner(const DrawEntry* entry){
    /* Pixel writes may alias any C object: snapshot immutable material fields
     * so the compiler need not reload them after every framebuffer store. */
    const unsigned mode = entry->mode;
    const uint8_t colour = entry->colour;
    int32_t x[3], y[3], u[3], v[3];
    for(unsigned i = 0; i < 3; i++){
        x[i] = screen_x(entry->s[i]);
        y[i] = screen_y(entry->s[i]);
        u[i] = (int16_t)entry->uv[i];
        v[i] = (int16_t)(entry->uv[i] >> 16);
    }

    int64_t signed_area = y_raster_edge(x[0], y[0], x[1], y[1], x[2], y[2]);
    if(!signed_area) return;
    bool reverse = signed_area < 0;
    int64_t area = reverse ? -signed_area : signed_area;

    int32_t min_x = x[0], max_x = x[0], min_y = y[0], max_y = y[0];
    for(unsigned i = 1; i < 3; i++){
        if(x[i] < min_x) min_x = x[i];
        if(x[i] > max_x) max_x = x[i];
        if(y[i] < min_y) min_y = y[i];
        if(y[i] > max_y) max_y = y[i];
    }
    int first_x = (min_x >> 4) - 1;
    int last_x = (max_x >> 4) + 1;
    int first_y = (min_y >> 4) - 1;
    int last_y = (max_y >> 4) + 1;
    if(first_x < 0) first_x = 0;
    if(first_y < 0) first_y = 0;
    if(last_x >= FRAME_WIDTH) last_x = FRAME_WIDTH - 1;
    if(last_y >= FRAME_HEIGHT) last_y = FRAME_HEIGHT - 1;
    if(first_x > last_x || first_y > last_y) return;

    int64_t du_dx_num = (int64_t)(u[1] - u[0]) * (y[2] - y[0]) -
                        (int64_t)(u[2] - u[0]) * (y[1] - y[0]);
    int64_t du_dy_num = (int64_t)(x[1] - x[0]) * (u[2] - u[0]) -
                        (int64_t)(x[2] - x[0]) * (u[1] - u[0]);
    int64_t dv_dx_num = (int64_t)(v[1] - v[0]) * (y[2] - y[0]) -
                        (int64_t)(v[2] - v[0]) * (y[1] - y[0]);
    int64_t dv_dy_num = (int64_t)(x[1] - x[0]) * (v[2] - v[0]) -
                        (int64_t)(x[2] - x[0]) * (v[1] - v[0]);
    bool textured = mode != RM_FLAT && mode != RM_TINT;
    if(textured && !entry->texture) return;
    /* Depth collection is independent of the optional depth test. Flat and
     * blending passes retain their original sampling; correct world/cutouts. */
    const bool perspective = g_perspective_active && g_raster_depth &&
        (mode == RM_TEX || mode == RM_ALPHA) &&
        g_raster_depth[0] && g_raster_depth[1] && g_raster_depth[2];
    const bool affine = textured && !perspective;
    const bool setup_recip = y_g_raster_setup_recip_enabled;
    uint64_t reciprocal = 0;
    int64_t du_dx = affine ? y_raster_setup_div(du_dx_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t du_dy = affine ? y_raster_setup_div(du_dy_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t dv_dx = affine ? y_raster_setup_div(dv_dx_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    int64_t dv_dy = affine ? y_raster_setup_div(dv_dy_num * 16 * 65536, signed_area, &reciprocal, setup_recip) : 0;
    /* Model UVs use 16 units per texel; the original mip threshold is Q8. */
    /* abs(trunc(num*256/area)) >=1536 iff abs(num)>=6*abs(area).
     * Screen and UV inputs are signed 16-bit, so these products fit int64. */
    bool mip1 = entry->texture && entry->texture->mip[1] != entry->texture->mip[0] &&
                (y_raster_abs64(du_dx_num) >= (uint64_t)area * 6 ||
                 y_raster_abs64(du_dy_num) >= (uint64_t)area * 6 ||
                 y_raster_abs64(dv_dx_num) >= (uint64_t)area * 6 ||
                 y_raster_abs64(dv_dy_num) >= (uint64_t)area * 6);

    double pq[3] = {0}, pu[3] = {0}, pv[3] = {0};
    if(perspective) for(unsigned i = 0; i < 3; i++){
        pq[i] = g_raster_depth[i];
        pu[i] = pq[i] * u[i]; pv[i] = pq[i] * v[i];
    }
    int orientation = reverse ? -1 : 1;
    int64_t edge1_x = orientation * (int64_t)(y[2] - y[0]) * 16;
    int64_t edge1_y = orientation * -(int64_t)(x[2] - x[0]) * 16;
    int64_t edge2_x = orientation * -(int64_t)(y[1] - y[0]) * 16;
    int64_t edge2_y = orientation * (int64_t)(x[1] - x[0]) * 16;
    int32_t start_sx = first_x * 16 + 8;
    int32_t start_sy = first_y * 16 + 8;
    int64_t w1_start = orientation * -y_raster_edge(x[0], y[0], x[2], y[2], start_sx, start_sy);
    int64_t w2_start = orientation * y_raster_edge(x[0], y[0], x[1], y[1], start_sx, start_sy);
    int64_t u_start = perspective ? 0 : (int64_t)u[0] * 65536 +
                      (du_dx * (start_sx - x[0]) + du_dy * (start_sy - y[0])) / 16;
    int64_t v_start = perspective ? 0 : (int64_t)v[0] * 65536 +
                      (dv_dx * (start_sx - x[0]) + dv_dy * (start_sy - y[0])) / 16;

    /* Reciprocal depth is affine in screen space. Vertices retain seven
     * fractional bits beyond the 16-bit buffer; eight more setup bits keep
     * per-pixel increments accurate without a division in the pixel loop. */
    const bool depth_test = g_depth_active && g_raster_depth;
    int32_t depth_dx = 0, depth_dy = 0;
    uint32_t depth_start = 0;
    if(depth_test){
        int64_t d0 = g_raster_depth[0];
        int64_t d1 = (int64_t)g_raster_depth[1] - d0;
        int64_t d2 = (int64_t)g_raster_depth[2] - d0;
        int64_t dx = y_raster_setup_div((d1 * (y[2] - y[0]) - d2 * (y[1] - y[0])) * 4096,
                                      signed_area, &reciprocal, setup_recip);
        int64_t dy = y_raster_setup_div(((x[1] - x[0]) * d2 - (x[2] - x[0]) * d1) * 4096,
                                      signed_area, &reciprocal, setup_recip);
        depth_dx = (int32_t)dx;
        depth_dy = (int32_t)dy;
        depth_start = (uint32_t)(d0 * 256 +
            (dx * (start_sx - x[0]) + dy * (start_sy - y[0])) / 16);
    }

    extern uint8_t g_shade_water[256];
    const uint8_t* texture = NULL;
    if(entry->texture)
        texture = (const uint8_t*)(uintptr_t)entry->texture->mip[mip1 ? 1 : 0];
    uint8_t* framebuffer = g_engine_fb;
    SMG_COUNT(raster_triangles);
    if(!textured) SMG_COUNT(flat_triangles);
    else if(mip1) SMG_COUNT(mip1_triangles);
    else SMG_COUNT(mip0_triangles);
    int64_t w1_row = w1_start, w2_row = w2_start;
    uint32_t u_row = (uint32_t)u_start, v_row = (uint32_t)v_start;
    uint32_t depth_row = depth_start;
    double perspective_row[3] = {0}, perspective_dx[3] = {0}, perspective_dy[3] = {0};
    if(perspective){
        const double* attributes[3] = {pq, pu, pv};
        for(unsigned i = 0; i < 3; i++){
            const double* a = attributes[i];
            double d1 = a[1] - a[0], d2 = a[2] - a[0];
            perspective_row[i] = area * a[0] + w1_start * d1 + w2_start * d2;
            perspective_dx[i] = edge1_x * d1 + edge2_x * d2;
            perspective_dy[i] = edge1_y * d1 + edge2_y * d2;
        }
    }
    for(int py = first_y; py <= last_y; py++){
        int left = 0, right = last_x - first_x;
        bool visible = y_raster_span_bound(w1_row, edge1_x, &left, &right) &&
                       y_raster_span_bound(w2_row, edge2_x, &left, &right) &&
                       y_raster_span_bound(area - w1_row - w2_row, -edge1_x - edge2_x, &left, &right);
        uint32_t uw = u_row + (uint32_t)du_dx * (unsigned)left;
        uint32_t vw = v_row + (uint32_t)dv_dx * (unsigned)left;
        uint32_t depth_value = depth_row + (uint32_t)depth_dx * (unsigned)left;
        double q = 0, uq = 0, vq = 0, dq = 0, duq = 0, dvq = 0;
        y_PerspectiveSpan span = {0};
        if(visible && perspective){
            dq = perspective_dx[0]; duq = perspective_dx[1]; dvq = perspective_dx[2];
            q = perspective_row[0] + dq * left;
            uq = perspective_row[1] + duq * left;
            vq = perspective_row[2] + dvq * left;
        }
        if(visible) for(int px = first_x + left; px <= first_x + right; px++){
                if(perspective && !span.count){
                    span = y_raster_perspective_span(q, uq, vq, dq, duq, dvq,
                        (unsigned)(first_x + right - px + 1));
                    q += dq * span.count; uq += duq * span.count; vq += dvq * span.count;
                    uw = span.u; vw = span.v;
                }
                uint8_t* pixel = framebuffer + py * FRAME_STRIDE + px;
                uint16_t* depth_pixel = NULL;
                uint32_t depth = 0;
                if(depth_test){
                    depth_pixel = &g_depth_buffer[py * FRAME_WIDTH + px];
                    depth = (int32_t)depth_value > 0 ? depth_value >> 15 : 1;
                    if(!depth) depth = 1;
                    if(depth > 65535) depth = 65535;
                    unsigned bias = mode == RM_DARKEN ? 2 : 0;
                    if(depth + bias < *depth_pixel) goto pixel_done;
                }
                uint8_t source = 0;
                if(textured){
                    /* Only bits 20..24 of Q16 UV accumulators select texels;
                     * unsigned 32-bit wrapping preserves them exactly. */
                    /* Assets and the GBA TEXEL macro store V rows, U columns.
                     * Mip1 halves the row count, retaining all 32 columns. */
                    unsigned row = mip1 ? (vw >> 21) & 15 : (vw >> 20) & 31;
                    source = texture[row * (mip1 ? TEX_MIP1_STRIDE : TEX_DIM) + ((uw >> 20) & 31)];
                }

                switch(mode){
                case RM_FLAT:
                case RM_TEX:
                    *pixel = mode == RM_FLAT ? colour : source;
                    if(depth_pixel) *depth_pixel = (uint16_t)depth;
                    break;
                case RM_ALPHA:
                    if(source){
                        *pixel = source;
                        if(depth_pixel) *depth_pixel = (uint16_t)depth;
                    }
                    break;
                case RM_ADD:
                    if(source && g_additive_rows && g_additive_table){
                        uint32_t index = (uint32_t)*pixel + ((uint32_t)g_additive_rows[source] << 8);
                        *pixel = g_additive_table[index];
                    }
                    break;
                case RM_DARKEN:
                    if(source) *pixel = shade_darken[*pixel];
                    break;
                case RM_TINT:
                    *pixel = g_shade_water[*pixel];
                    break;
                default:
                    break;
                }
pixel_done:
            uw += perspective ? span.du : (uint32_t)du_dx;
            vw += perspective ? span.dv : (uint32_t)dv_dx;
            if(perspective) span.count--;
            depth_value += (uint32_t)depth_dx;
        }
        w1_row += edge1_y;
        w2_row += edge2_y;
        u_row += (uint32_t)du_dy;
        v_row += (uint32_t)dv_dy;
        depth_row += (uint32_t)depth_dy;
        if(perspective) for(unsigned i = 0; i < 3; i++) perspective_row[i] += perspective_dy[i];
    }
}
