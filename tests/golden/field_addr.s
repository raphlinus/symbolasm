.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global fields
.type fields, %function
.thumb_func
fields:
    add r2, r0, #28
    ldr r3, [r2, r1, lsl #2]
    adds r2, r0, #4
    ldr r3, [r2, #4]
    add r2, r0, #12
    mov r2, r0
    bx lr
.size fields, . - fields
.section .text
.global statics
.type statics, %function
.thumb_func
statics:
    ldr r0, =state
    ldr r1, =state+28
    ldr r2, =state+12
    ldr r3, =state+4
    bx lr
.size statics, . - statics
.section .bss
.balign 4
state:
    .space 36
