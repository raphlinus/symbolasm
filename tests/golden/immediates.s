.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global fit
.thumb_func
fit:
    add r0, r0, #4095
    sub r0, r0, #4096
    add r0, r0, #16
    bic r1, r0, #255
    bic r1, r0, #255
    orn r1, r0, #1
    cmp r0, #4294967295
    cmp r0, #16
    subs r1, r1, #4096
    adds r1, r1, #4278190080
    mov r2, #4278255360
    ldr r2, [r0, #-4]
    ldr r2, [r0, #4095]
    bx lr
