.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global solid_render
.type solid_render, %function
.thumb_func
solid_render:
    bx lr
.size solid_render, . - solid_render
.section .text
.balign 4
solid_vtable:
    .word solid_render
    .word 0x0
.section .data
.balign 4
objs:
    .word solid_vtable
    .hword 0x0
    .hword 0x0
    .hword 0x280
    .hword 0x140
    .word 0x6318
    .word solid_vtable
    .hword 0x0
    .hword 0x140
    .hword 0x5c
    .hword 0x168
    .word 0x18
.section .text
.balign 4
padded:
    .byte 0x1
    .space 3
    .word 0x2
    .hword 0x3
    .space 2
    .byte 0x1
    .space 3
    .word 0x2
    .hword 0x3
    .space 2
.section .data
.balign 4
one:
    .byte 0x4
    .space 3
    .word 0x5
    .hword 0x6
    .space 2
