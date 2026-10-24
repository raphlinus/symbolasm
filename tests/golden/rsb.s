.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global rsb
.type rsb, %function
.thumb_func
rsb:
    rsb r2, r0, #32
    rsb r2, r1, #4096
    rsb r2, r0, r1, lsl #3
    rsbs r0, r0, #0
    rsbs r2, r1, #5
    bx lr
.size rsb, . - rsb
