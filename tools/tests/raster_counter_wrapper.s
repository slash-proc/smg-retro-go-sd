.syntax unified
.cpu cortex-m7
.thumb
.text
.global raster_counter_call
.thumb_func
raster_counter_call:
    push {r4-r6, lr}
    mov r4, r2
    ldr r5, =0xe0001004
    ldr r6, [r5]
    str r6, [r4]
    blx r1
    ldr r6, [r5]
    str r6, [r4, #4]
    pop {r4-r6, pc}
.global raster_counter_empty
.thumb_func
raster_counter_empty:
    bx lr
.ltorg
