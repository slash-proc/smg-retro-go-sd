#if !defined(GALAXY_RAW_TEXTURE_REFERENCES) || GALAXY_RAW_TEXTURE_REFERENCES != 2
#error "assets use an old texture reference format, rebuild them"
#endif

static Game g_game EWRAM_BSS;

static uint16_t g_debug_options EWRAM_BSS;

static const ViewMode view_modes[VIEW_MODE_COUNT] = {
    {FX_ONE, false, 0x7FFFFFFF, 0, false, VIEW_MAX_ACTORS, 12000, FXC(120.0f), 0, 0},

    {FXC(0.75f), true, 28000, 38, true, 16, 5000, FXC(80.0f), 8, 1600},

    {FXC(0.5f), true, 14000, 19, true, 10, 3200, FXC(50.0f), 6, 640},

    {FXC(0.5f), false, 14000, 19, true, 10, 3200, FXC(50.0f), 6, 640},

    {FXC(0.3f), false, 14000, 19, true, 8, 2400, FXC(40.0f), 4, 480},
};

static inline const ViewMode* view_mode(void){ return &view_modes[g_game.view_distance]; }
static inline const ViewMode* view_cinematic_detail(void){
    /* Full is unbounded main geometry and can overflow during descent.
     * High plus textured far geometry keeps the whole destination visible. */
    return &view_modes[1];
}

static const fx camera_zooms[CAMERA_ZOOM_COUNT] = {FX_ONE, FXC(0.8f), FXC(0.62f), FXC(0.46f)};
