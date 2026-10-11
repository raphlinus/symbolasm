.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global index
.thumb_func
index:
    ldr r2, [r0, r1, lsl #2]
    str r2, [r0, r1, lsl #2]
    ldr r3, [r0, r1, lsl #2]
    ldrh r2, [r0, r1, lsl #1]
    ldrb r3, [r0, r1]
    lsl r3, r1, #3
    lsl r3, r1, #1
    and r3, r1, #255
    ldr r2, [r0, #8]
    add r0, r0, #12
    bx lr
