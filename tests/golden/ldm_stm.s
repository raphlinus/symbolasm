.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global copy
.thumb_func
copy:
    ldm r0!, {r3}
    stm r1!, {r3}
    ldr r8, [r0], #4
    str r8, [r1], #4
    ldrh r2, [r0], #4
    ldr r3, [r0], #8
    bx lr
