.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global use_data
.thumb_func
use_data:
    ldr r1, =line
    ldr r2, [r1, r0, lsl #2]
    mov r3, #321
    mov r3, #24
    mov r3, #1292
    ldr r3, magic
    ldr r1, =pairs
    ldrb r2, [r1, #4]
    mov r3, #321
    bx lr
.section .bss
.balign 4
line:
    .space 1284
.section .bss
.balign 4
pairs:
    .space 24
.section .data
.balign 4
cmds:
    .word 0x1010
    .word 0x354d5354
    .word 0x3280
    .word 0x7c00
.section .data
.balign 4
fill:
    .fill 11, 4, 0x7c007c00
.section .data
.balign 1
bytes:
    .byte 0x1
    .byte 0xff
    .byte 0xff
    .byte 0x0
.section .data
.balign 2
halves:
    .hword 0xfffe
    .hword 0x7fff
.section .data
.balign 4
ptrs:
    .word _stack_top
    .word line
    .word line + 0x4
.section .data
.balign 4
counter:
    .word 0x5
.section .data
.balign 1
grid:
    .byte 0x1
    .byte 0x2
    .byte 0x3
    .byte 0x4
.section .text
.balign 4
magic:
    .word 0xabcd1234
