.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global f
.type f, %function
.thumb_func
f:
    uxth r0, r0
    uxtb r0, r0
    bfc r0, #4, #28
    bfc r9, #16, #16
    bx lr
.size f, . - f
