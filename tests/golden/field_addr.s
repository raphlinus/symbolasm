.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global fields
.thumb_func
fields:
    add r2, r0, #28
    ldr r3, [r2, r1, lsl #2]
    add r2, r0, #4
    ldr r3, [r2, #4]
    add r2, r0, #12
    mov r2, r0
    bx lr
.section .text
.global statics
.thumb_func
statics:
    ldr r0, =state
    ldr r1, =state+28
    ldr r2, =state+12
    ldr r3, =state+4
    bx lr
.section .bss
.balign 4
state:
    .space 36
