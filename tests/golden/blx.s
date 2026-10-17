.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global dispatch
.type dispatch, %function
.thumb_func
dispatch:
    push {r4, lr}
    ldr r1, [r0, #0]
    ldr r4, [r1, #4]
    blx r4
    movs r0, #0
    mov r3, r4
    blx r3
    pop {r4, pc}
.size dispatch, . - dispatch
