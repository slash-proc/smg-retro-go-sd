#if defined(__GNW__)
#define POSE_DECODE_FN __attribute__((noinline, used))
#else
#define POSE_DECODE_FN __attribute__((target("arm"), noinline, used, section(".posecodec")))
#endif
POSE_DECODE_FN static const int16_t* pose_decode(const uint8_t* descriptor, int16_t* out){
    const PackedPoseHeader* h = (const PackedPoseHeader*)(const void*)descriptor;
    const uint8_t* data = descriptor + sizeof(*h);
    if(h->mode == FIGURE_POSE_RAW) return (const int16_t*)(const void*)data;
    const int16_t* bind = (const int16_t*)(const void*)(descriptor + h->bind_relative);
    unsigned flagged_scale = 2 + (h->mode >> 1);
    unsigned remain = h->point_count;
    int16_t* decoded = out;
    while(remain){
        const int16_t* header = (const int16_t*)(const void*)data;
        int minimum[3] = {header[0], header[1], header[2]};
        unsigned packed = (uint16_t)header[3];
        unsigned scale = (packed >> 15) * flagged_scale;
        unsigned width[3] = {packed & 31u, (packed >> 5) & 31u, (packed >> 10) & 31u};
        unsigned mask[3] = {(1u << width[0]) - 1u, (1u << width[1]) - 1u, (1u << width[2]) - 1u};
        unsigned count = remain > FIGURE_POSE_BLOCK ? FIGURE_POSE_BLOCK : remain;
        const uint32_t* words = (const uint32_t*)(const void*)(data + 8);
        uint32_t bits = 0;
        unsigned available = 0;
        for(unsigned i = 0; i < count; i++){
#define POSE_COMPONENT(axis)                                                           \
    do {                                                                               \
        unsigned w = width[axis], value;                                               \
        if(available < w){                                                            \
            uint32_t next = *words++;                                                  \
            value = (bits | (next << available)) & mask[axis];                         \
            bits = next >> (w - available);                                            \
            available += 32 - w;                                                       \
        } else {                                                                       \
            value = bits & mask[axis];                                                 \
            bits >>= w;                                                                \
            available -= w;                                                            \
        }                                                                              \
        *out++ = (int16_t)((uint16_t)*bind++ + minimum[axis] + (int)(value << scale)); \
    } while(0)
            POSE_COMPONENT(0);
            POSE_COMPONENT(1);
            POSE_COMPONENT(2);
#undef POSE_COMPONENT
        }
        data += 8 + (count * (width[0] + width[1] + width[2]) + 7) / 8;
        remain -= count;
    }
    return decoded;
}
