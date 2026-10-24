.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global divmod
.type divmod, %function
.thumb_func
divmod:
    udiv r2, r0, r1
    mls r1, r2, r1, r0
    mov r0, r2
    bx lr
.size divmod, . - divmod
.section .text
.global render
.type render, %function
.thumb_func
render:
    movs r0, #1
    bx lr
.size render, . - render
.section .text
.global addrs
.type addrs, %function
.thumb_func
addrs:
    ldr r1, =divmod
    ldr r2, [r0, #0]
    bx lr
.size addrs, . - addrs
.section .text
.balign 4
vt:
    .word render
    .word 0x0
