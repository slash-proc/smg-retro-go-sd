#include <stddef.h>
#include <stdint.h>
#include <string.h>

typedef struct Block Block;
struct Block {
    size_t size;
    Block *previous;
    Block *next;
    Block *free_previous;
    Block *free_next;
    uint32_t is_free;
    uint32_t reserved;
};

extern unsigned char __heap_base;

static Block *first_block;
static Block *last_block;
static Block *free_bins[32];
static uintptr_t heap_end;
#ifdef SMG_ASSETC_PROGRESS
static uint32_t malloc_calls;
extern void smg_assetc_progress(uint32_t phase, uint32_t index, uint32_t total);
#endif

static unsigned size_bin(size_t size) {
    unsigned bin = 0;
    size >>= 4;
    while (size > 1 && bin < 31) {
        size >>= 1;
        bin++;
    }
    return bin;
}

static void free_remove(Block *block) {
    unsigned bin = size_bin(block->size);
    if (block->free_previous) block->free_previous->free_next = block->free_next;
    else free_bins[bin] = block->free_next;
    if (block->free_next) block->free_next->free_previous = block->free_previous;
    block->free_previous = block->free_next = NULL;
}

static void free_insert(Block *block) {
    unsigned bin = size_bin(block->size);
    block->is_free = 1;
    block->free_previous = NULL;
    block->free_next = free_bins[bin];
    if (block->free_next) block->free_next->free_previous = block;
    free_bins[bin] = block;
}

static uintptr_t align_up(uintptr_t value, uintptr_t alignment) {
    return (value + alignment - 1) & ~(alignment - 1);
}

static int ensure_heap(uintptr_t required_end) {
    uint32_t pages = __builtin_wasm_memory_size(0);
    uintptr_t current_end = (uintptr_t)pages * 65536u;
    if (!heap_end) {
        heap_end = align_up((uintptr_t)&__heap_base, 16);
        if (heap_end > current_end) return 0;
    }
    if (required_end <= current_end) return 1;
    uintptr_t missing = required_end - current_end;
    uint32_t additional_pages = (uint32_t)((missing + 65535) / 65536);
    if (__builtin_wasm_memory_grow(0, additional_pages) == (size_t)-1) return 0;
    return 1;
}

static void split_block(Block *block, size_t wanted) {
    if (block->size < wanted + sizeof(Block) + 16) return;
    Block *remainder = (Block *)((uint8_t *)(block + 1) + wanted);
    remainder->size = block->size - wanted - sizeof(Block);
    remainder->previous = block;
    remainder->next = block->next;
    remainder->is_free = 0;
    remainder->free_previous = remainder->free_next = NULL;
    remainder->reserved = 0;
    if (remainder->next) remainder->next->previous = remainder;
    else last_block = remainder;
    block->next = remainder;
    block->size = wanted;
    if (remainder->next && remainder->next->is_free) {
        Block *next = remainder->next;
        free_remove(next);
        remainder->size += sizeof(Block) + next->size;
        remainder->next = next->next;
        if (remainder->next) remainder->next->previous = remainder;
        else last_block = remainder;
    }
    free_insert(remainder);
}

void *malloc(size_t size) {
#ifdef SMG_ASSETC_PROGRESS
    malloc_calls++;
    if (malloc_calls % 10000 == 0)
        smg_assetc_progress(40, malloc_calls, (uint32_t)size);
#endif
    size = (size + 15u) & ~(size_t)15u;
    if (!size) size = 16;

    for (unsigned bin = size_bin(size); bin < 32; bin++) {
        unsigned scanned = 0;
        for (Block *block = free_bins[bin]; block; block = block->free_next) {
#ifdef SMG_ASSETC_PROGRESS
            if (++scanned > 1000000) {
                smg_assetc_progress(41, bin, scanned);
                __builtin_trap();
            }
#endif
            if (block->size < size) continue;
            free_remove(block);
            block->is_free = 0;
            split_block(block, size);
            return block + 1;
        }
    }

    uintptr_t start = heap_end ? heap_end : align_up((uintptr_t)&__heap_base, 16);
    uintptr_t end = start + sizeof(Block) + size;
    if (end < start || !ensure_heap(end)) return NULL;
    Block *block = (Block *)start;
    block->size = size;
    block->previous = last_block;
    block->next = NULL;
    block->free_previous = block->free_next = NULL;
    block->is_free = 0;
    block->reserved = 0;
    if (last_block) last_block->next = block;
    else first_block = block;
    last_block = block;
    heap_end = end;
    return block + 1;
}

void free(void *pointer) {
    if (!pointer) return;
    Block *block = (Block *)pointer - 1;
    if (block->is_free) return;
    if (block->next && block->next->is_free) {
        Block *next = block->next;
        free_remove(next);
        block->size += sizeof(Block) + next->size;
        block->next = next->next;
        if (block->next) block->next->previous = block;
        else last_block = block;
    }
    if (block->previous && block->previous->is_free) {
        Block *previous = block->previous;
        free_remove(previous);
        previous->size += sizeof(Block) + block->size;
        previous->next = block->next;
        if (previous->next) previous->next->previous = previous;
        else last_block = previous;
        block = previous;
    }
    free_insert(block);
}

void *calloc(size_t count, size_t size) {
    if (size && count > SIZE_MAX / size) return NULL;
    size_t total = count * size;
    void *pointer = malloc(total);
    if (pointer) memset(pointer, 0, total);
    return pointer;
}

void *realloc(void *pointer, size_t size) {
    if (!pointer) return malloc(size);
    if (!size) {
        free(pointer);
        return NULL;
    }
    size = (size + 15u) & ~(size_t)15u;
    Block *block = (Block *)pointer - 1;
    if (block->size >= size) {
        split_block(block, size);
        return pointer;
    }
    if (block->next && block->next->is_free &&
        block->size + sizeof(Block) + block->next->size >= size) {
        Block *next = block->next;
        free_remove(next);
        block->size += sizeof(Block) + next->size;
        block->next = next->next;
        if (block->next) block->next->previous = block;
        else last_block = block;
        split_block(block, size);
        return pointer;
    }
    void *replacement = malloc(size);
    if (!replacement) return NULL;
    memcpy(replacement, pointer, block->size);
    free(pointer);
    return replacement;
}
