.cpu cortex-m33
.syntax unified
.thumb
.section .text
.balign 32
.global aligned
.type aligned, %function
.thumb_func
aligned:
    bx lr
.size aligned, . - aligned
.section .ram.in_ram,"ax",%progbits
.balign 8
.global in_ram
.type in_ram, %function
.thumb_func
in_ram:
    bx lr
.size in_ram, . - in_ram
.section .bss
.balign 8
heap:
    .space 64
.section .bss
.balign 4
table:
    .space 16
.section .ram.buf,"aw",%progbits
.balign 16
buf:
    .space 32
