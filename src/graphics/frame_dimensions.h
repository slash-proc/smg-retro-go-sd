#ifndef SMG_FRAME_DIMENSIONS_H
#define SMG_FRAME_DIMENSIONS_H

/* The world raster is independent of the 320x240 RGB565 HUD/compositor.
 * Keep the indexed stride large enough for the legacy 240x160 menus. */
#ifndef SMG_RENDER_WIDTH
#define SMG_RENDER_WIDTH 160
#endif
#ifndef SMG_RENDER_HEIGHT
#define SMG_RENDER_HEIGHT 120
#endif

#define FRAME_WIDTH SMG_RENDER_WIDTH
#define FRAME_HEIGHT SMG_RENDER_HEIGHT
#define FRAME_STRIDE 320

#if FRAME_WIDTH <= 0 || FRAME_WIDTH > FRAME_STRIDE || FRAME_HEIGHT <= 0 || FRAME_HEIGHT > 240
#error "SMG raster dimensions exceed the indexed framebuffer"
#endif

#endif
