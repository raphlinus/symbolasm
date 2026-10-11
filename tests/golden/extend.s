.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global extend
.type extend, %function
.thumb_func
extend:
    uxtb r2, r0
    uxth r2, r0
    sxtb r2, r1
    sxth r2, r1
    ubfx r2, r0, #8, #8
    sbfx r2, r1, #8, #8
    bx lr
.size extend, . - extend
