#include <assert.h>
#include <stdint.h>
#include <stdio.h>

uint32_t isqrt32_arm(uint32_t value);
uint32_t recip30_arm(uint32_t value);

static uint32_t reference_sqrt(uint32_t value)
{
    uint64_t low = 0;
    uint64_t high = 65536;
    while (low + 1 < high) {
        uint64_t mid = (low + high) / 2;
        if (mid * mid <= value) low = mid;
        else high = mid;
    }
    return (uint32_t)low;
}

static uint32_t reference_recip(uint32_t value)
{
    if (value == 0) return 0x1ffffu;
    uint64_t quotient = (1ull << 30) / value;
    return quotient > 0x1ffffu ? 0x1ffffu : (uint32_t)quotient;
}

int main(void)
{
    for (uint32_t value = 0; value < 1000000; ++value) {
        assert(isqrt32_arm(value) == reference_sqrt(value));
        assert(recip30_arm(value) == reference_recip(value));
    }

    uint32_t state = 0x51a7c3d9u;
    for (unsigned i = 0; i < 100000; ++i) {
        state = state * 1664525u + 1013904223u;
        assert(isqrt32_arm(state) == reference_sqrt(state));
        assert(recip30_arm(state) == reference_recip(state));
    }
    puts("ARM-to-Thumb math compatibility: 1,100,000 input pairs passed");
    return 0;
}
