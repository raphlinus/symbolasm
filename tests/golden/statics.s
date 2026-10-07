.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global bump
.thumb_func
bump:
    ldr r0, =dvi
    ldr r1, [r0, #4]
    eor r1, r1, #1
    str r1, [r0, #4]
    ldr r2, =counter
    ldrh r3, [r2]
    add r3, r3, #1
    strh r3, [r2]
    bx lr
.section .text
.global folding
.thumb_func
folding:
    ldr r0, =34603264
    mov r1, #5396
    orr r2, r1, #65536
    ldr r3, =1073971252
    bx lr
.section .text
.global compound
.thumb_func
compound:
    and r0, r0, r1
    orr r0, r0, #16
    eor r0, r0, r1
    lsl r0, r0, #2
    lsr r0, r0, r1
    bic r0, r0, r1
    bx lr
.section .bss
.balign 4
dvi:
    .space 12
.section .bss
.balign 2
counter:
    .space 2
