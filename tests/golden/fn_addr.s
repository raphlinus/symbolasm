.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global render
.type render, %function
.thumb_func
render:
    bx lr
.size render, . - render
.section .text
.global start
.type start, %function
.thumb_func
start:
    bx lr
.size start, . - start
.section .text
.balign 4
vtable:
    .word render
    .word 0x0
.section .text
.balign 4
table:
    .word render
    .word start
    .word start + 0x4
