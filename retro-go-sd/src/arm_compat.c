/* Thumb-2 replacements for small ARM-state math helpers used by the GBA TU. */
#include <stdint.h>

uint32_t isqrt32_arm(uint32_t value)
{
    uint32_t root = 0;
    uint32_t bit = 1u << 30;

    while (bit > value) bit >>= 2;
    while (bit != 0) {
        if (value >= root + bit) {
            value -= root + bit;
            root = (root >> 1) + bit;
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    return root;
}

uint32_t recip30_arm(uint32_t value)
{
    if (value == 0) return 0x1ffffu;

    uint32_t remainder = 1u << 30;
    uint32_t quotient = 0;
    for (int shift = 16; shift >= 0; --shift) {
        uint64_t divisor = (uint64_t)value << shift;
        if (remainder >= divisor) {
            remainder -= (uint32_t)divisor;
            quotient |= 1u << shift;
        }
    }
    return quotient;
}
