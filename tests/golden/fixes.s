.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global div
.thumb_func
div:
    udiv r2, r0, r1
    sdiv r3, r0, r1
    bx lr
.section .text
.global tuple3
.thumb_func
tuple3:
    mov r1, #1
    mov r2, #2
    mov r3, #3
    stm r0, {r1, r2, r3}
    ldm r0!, {r1, r2, r3}
    bx lr
.section .text
.global assoc
.thumb_func
assoc:
    sub r2, r0, r1, lsl #2
    bx lr
.section .text
.global branch_named_vars
.thumb_func
branch_named_vars:
    mov r1, r0
    add r1, r1, #1
    mov r2, r1
    cbz r2, out
    mov r0, r2
out:
    bx lr
.section .text
.global slices
.thumb_func
slices:
    sbfx r1, r0, #4, #8
    ubfx r2, r0, #4, #8
    bx lr
.section .text
.global placed_flags
.thumb_func
placed_flags:
    adds r2, r0, r1
    add r2, r2, #1
    bx lr
