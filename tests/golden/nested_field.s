.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global nested
.type nested, %function
.thumb_func
nested:
    ldr r1, [r0, #16]
    adds r1, r1, #1
    str r1, [r0, #16]
    ldr r2, [r0, #0]
    ldr r3, [r0, #20]
    str r3, [r0, #8]
    bx lr
.size nested, . - nested
