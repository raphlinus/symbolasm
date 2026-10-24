.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global set_x
.type set_x, %function
.thumb_func
set_x:
    movs r1, #0
    str r1, [r0, #0]
    bx lr
.size set_x, . - set_x
