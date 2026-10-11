.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global calls
.thumb_func
calls:
    clz r2, r0
    rbit r2, r0
    rev r2, r0
    rev16 r2, r0
    revsh r2, r0
    ror r2, r0, #8
    ror r2, r0, r1
    pkhbt r2, r0, r1, lsl #16
    pkhbt r2, r0, r1
    pkhtb r2, r0, r1, asr #16
    rors r2, r0, #1
    bx lr
