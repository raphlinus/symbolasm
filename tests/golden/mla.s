.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global mla
.type mla, %function
.thumb_func
mla:
    mla r3, r1, r2, r0
    mla r3, r2, r1, r0
    mla r0, r1, r2, r0
    mls r0, r1, r2, r0
    add r3, r0, r1, lsl #2
    bx lr
.size mla, . - mla
.section .text
.global line_ptr
.type line_ptr, %function
.thumb_func
line_ptr:
    mov r2, #1284
    mla r0, r1, r2, r0
    bx lr
.size line_ptr, . - line_ptr
