enum { ML_WAIT = 64, ML_READY, ML_PLAYERIN, ML_PREP, ML_AIM, ML_FLIGHT, ML_KILL, ML_LAND, ML_REJECT };

static const FigureOps mario_launcher_figure_ops;

static bool mario_launcher_bound(void);
static bool mario_launcher_aiming(void);
static void mario_launcher_gate_init(Actor* a, const StageObject* o);
static void shutter_think(Actor* a);
static fx shutter_gate_draw_extent(const Actor* actor);
static void mario_launcher_think(Actor* a);
static bool mario_launcher_camera(fx dt, const KeyState* keys);
