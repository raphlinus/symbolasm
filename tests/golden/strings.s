.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global lengths
.type lengths, %function
.thumb_func
lengths:
    movs r0, #13
    movs r0, #19
    movs r0, #12
    ldr r1, =avg_str
    ldrb r2, [r1]
    bx lr
.size lengths, . - lengths
.section .text
.balign 1
avg_str:
    .ascii "cycles/line avg "
.section .text
.balign 1
esc_str:
    .ascii "a\011b\012\"q\" \\ \000\377\000"
.section .text
.balign 1
utf8_str:
    .ascii "\302\265s"
.section .data
.balign 1
buf:
    .ascii "edit me"
.section .text
.balign 4
words:
    .word 0x1
    .word 0x2
    .word 0x3
