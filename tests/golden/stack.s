.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global locals
.type locals, %function
.thumb_func
locals:
    push {r4, lr}
    sub sp, sp, #16
    str r0, [sp, #0]
    strh r1, [sp, #8]
    add r0, sp, #4
    bl use_point
    ldr r1, [sp, #0]
    ldrh r2, [sp, #8]
    adds r0, r1, r2
    add sp, sp, #16
    pop {r4, pc}
.size locals, . - locals
.section .text
.global keep
.type keep, %function
.thumb_func
keep:
    push {r0, lr}
    ldr r1, [sp, #0]
    adds r0, r1, #1
    pop {r0, pc}
.size keep, . - keep
.section .text
.global sixth
.type sixth, %function
.thumb_func
sixth:
    ldr r0, [sp, #0]
    ldr r1, [sp, #4]
    adds r0, r0, r1
    bx lr
.size sixth, . - sixth
.section .text
.global writeback
.type writeback, %function
.thumb_func
writeback:
    push {r4, r5, r6, lr}
    ldr r0, [sp], #4
    ldm sp!, {r1, r2}
    pop {pc}
.size writeback, . - writeback
.section .text
.global writeback_by
.type writeback_by, %function
.thumb_func
writeback_by:
    push {r4, r5, lr}
    ldr r0, [sp], #8
    pop {pc}
.size writeback_by, . - writeback_by
