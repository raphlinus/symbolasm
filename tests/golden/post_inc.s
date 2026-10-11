.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global post_inc
.thumb_func
post_inc:
    ldm r0!, {r3}
    stm r0!, {r3}
    str r3, [r0], #-4
    ldrb r3, [r1], #1
    strb r3, [r1], #-1
    ldrsh r3, [r2], #2
    strh r3, [r2], #-2
    ldm r0!, {r4, r5}
    stm r0!, {r4, r5}
    bx lr
