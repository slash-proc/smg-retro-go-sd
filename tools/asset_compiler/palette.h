#ifndef ASSETC_PALETTE_H
#define ASSETC_PALETTE_H

#include <stdint.h>

typedef struct {
    uint8_t r, g, b, a;
} Rgba;

int palette_nearest_opaque(const Rgba *pal, const float col[3]);
void palette_build_quarter_blend(uint8_t out[65536], const Rgba pal[256]);

typedef struct ColorMap ColorMap;
ColorMap *colormap_create(const Rgba *pal, int n);
int colormap_nearest_rgb(const ColorMap *map, uint8_t r, uint8_t g, uint8_t b);

#endif
