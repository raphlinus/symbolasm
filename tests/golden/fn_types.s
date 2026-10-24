.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global divmod
.type divmod, %function
.thumb_func
divmod:
    bx lr
.size divmod, . - divmod
.section .text
.global render
.type render, %function
.thumb_func
render:
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
