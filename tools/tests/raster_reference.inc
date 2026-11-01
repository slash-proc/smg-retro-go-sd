/* Pre-optimization scalar rasterizer. Texture axes corrected against the
 * asset format/GBA TEXEL contract; the independent contract test covers this. */
static int64_t reference_edge(int32_t ax, int32_t ay, int32_t bx, int32_t by, int32_t px,
                           int32_t py){
    return (int64_t)(bx - ax) * (py - ay) - (int64_t)(by - ay) * (px - ax);
}

static uint64_t reference_abs64(int64_t value){
    return (uint64_t)(value < 0 ? -value : value);
}

/* Model the GBA reciprocal with division, independently of its lookup table. */
static bool reference_mip_gradient(int64_t numerator, uint32_t denominator){
    int exponent = 0;
    while(denominator >= 512){ denominator >>= 1; exponent++; }
    while(denominator < 256){ denominator <<= 1; exponent--; }
    uint32_t reciprocal = denominator == 256 ? 65535 :
        (16777216u + denominator / 2) / denominator;
    unsigned shift = (unsigned)(exponent + 16);
    if(shift >= 32){ numerator >>= shift - 31; shift = 31; }
    int64_t scaled = (numerator * reciprocal) >> shift;
    return reference_abs64(scaled) >= 1536;
}

static uint8_t reference_texel(const TextureReference* reference, bool mip1, int32_t u, int32_t v){
    const uint8_t* texels = (const uint8_t*)(uintptr_t)reference->mip[mip1 ? 1 : 0];
    if(mip1){
        unsigned row = ((uint32_t)v >> 5) & 15;
        unsigned column = ((uint32_t)u >> 4) & 31;
        return texels[row * TEX_MIP1_STRIDE + column];
    }
    unsigned row = ((uint32_t)v >> 4) & 31;
    unsigned column = ((uint32_t)u >> 4) & 31;
    return texels[row * TEX_DIM + column];
}

__attribute__((noinline)) static void reference_raster(const DrawEntry* entry){
    int32_t x[3], y[3], u[3], v[3];
    for(unsigned i = 0; i < 3; i++){
        x[i] = screen_x(entry->s[i]);
        y[i] = screen_y(entry->s[i]);
        u[i] = (int16_t)entry->uv[i];
        v[i] = (int16_t)(entry->uv[i] >> 16);
    }

    int64_t signed_area = reference_edge(x[0], y[0], x[1], y[1], x[2], y[2]);
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
    int64_t du_dx = (du_dx_num * 16 * 65536) / signed_area;
    int64_t du_dy = (du_dy_num * 16 * 65536) / signed_area;
    int64_t dv_dx = (dv_dx_num * 16 * 65536) / signed_area;
    int64_t dv_dy = (dv_dy_num * 16 * 65536) / signed_area;
    bool mip1 = false;
    if(entry->texture && entry->texture->mip[1] != entry->texture->mip[0]){
        if(area <= INT32_MAX){
            mip1 = reference_mip_gradient(du_dx_num, (uint32_t)area) ||
                   reference_mip_gradient(dv_dx_num, (uint32_t)area);
        } else {
            mip1 = reference_abs64(du_dx_num) >= (uint64_t)area * 6 ||
                   reference_abs64(du_dy_num) >= (uint64_t)area * 6 ||
                   reference_abs64(dv_dx_num) >= (uint64_t)area * 6 ||
                   reference_abs64(dv_dy_num) >= (uint64_t)area * 6;
        }
    }

    int orientation = reverse ? -1 : 1;
    int64_t edge1_x = orientation * (int64_t)(y[2] - y[0]) * 16;
    int64_t edge1_y = orientation * -(int64_t)(x[2] - x[0]) * 16;
    int64_t edge2_x = orientation * -(int64_t)(y[1] - y[0]) * 16;
    int64_t edge2_y = orientation * (int64_t)(x[1] - x[0]) * 16;
    int32_t start_sx = first_x * 16 + 8;
    int32_t start_sy = first_y * 16 + 8;
    int64_t w1_start = orientation * -reference_edge(x[0], y[0], x[2], y[2], start_sx, start_sy);
    int64_t w2_start = orientation * reference_edge(x[0], y[0], x[1], y[1], start_sx, start_sy);
    int64_t u_start = ((int64_t)u[0] << 16) +
                      (du_dx * (start_sx - x[0]) + du_dy * (start_sy - y[0])) / 16;
    int64_t v_start = ((int64_t)v[0] << 16) +
                      (dv_dx * (start_sx - x[0]) + dv_dy * (start_sy - y[0])) / 16;

    extern uint8_t g_shade_water[256];
    const uint8_t* texture = NULL;
    if(entry->texture)
        texture = (const uint8_t*)(uintptr_t)entry->texture->mip[mip1 ? 1 : 0];
    uint8_t* framebuffer = g_engine_fb;
    for(int py = first_y; py <= last_y; py++){
        int64_t w1_row = w1_start + edge1_y * (py - first_y);
        int64_t w2_row = w2_start + edge2_y * (py - first_y);
        int64_t u_row = u_start + du_dy * (py - first_y);
        int64_t v_row = v_start + dv_dy * (py - first_y);
        for(int px = first_x; px <= last_x; px++){
            int offset = px - first_x;
            int64_t w1 = w1_row + edge1_x * offset;
            int64_t w2 = w2_row + edge2_x * offset;
            int64_t w0 = area - w1 - w2;
            if(w0 >= 0 && w1 >= 0 && w2 >= 0){
                uint8_t* pixel = framebuffer + py * FRAME_STRIDE + px;
                uint8_t source = 0;
                if(entry->mode != RM_FLAT && entry->mode != RM_TINT){
                    if(!texture) continue;
                    source = reference_texel(entry->texture, mip1, (int32_t)(u_row >> 16),
                                          (int32_t)(v_row >> 16));
                }

                switch(entry->mode){
                case RM_FLAT:
                case RM_TEX:
                    *pixel = entry->mode == RM_FLAT ? entry->colour : source;
                    break;
                case RM_ALPHA:
                    if(source) *pixel = source;
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
            }
            u_row += du_dx;
            v_row += dv_dx;
        }
    }
}
