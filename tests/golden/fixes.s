.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global div
.type div, %function
.thumb_func
div:
    udiv r2, r0, r1
    sdiv r3, r0, r1
    bx lr
.size div, . - div
.section .text
.global tuple3
.type tuple3, %function
.thumb_func
tuple3:
    mov r1, #1
    mov r2, #2
    mov r3, #3
    stm r0, {r1, r2, r3}
    ldm r0!, {r1, r2, r3}
    bx lr
.size tuple3, . - tuple3
.section .text
.global assoc
.type assoc, %function
.thumb_func
assoc:
    sub r2, r0, r1, lsl #2
    bx lr
.size assoc, . - assoc
.section .text
.global branch_named_vars
.type branch_named_vars, %function
.thumb_func
branch_named_vars:
    mov r1, r0
    add r1, r1, #1
    mov r2, r1
    cbz r2, .Lbranch_named_vars.out
    mov r0, r2
.Lbranch_named_vars.out:
    bx lr
.size branch_named_vars, . - branch_named_vars
.section .text
.global slices
.type slices, %function
.thumb_func
slices:
    sbfx r1, r0, #4, #8
    ubfx r2, r0, #4, #8
    bx lr
.size slices, . - slices
.section .text
.global placed_flags
.type placed_flags, %function
.thumb_func
placed_flags:
    adds r2, r0, r1
    add r2, r2, #1
    bx lr
.size placed_flags, . - placed_flags
