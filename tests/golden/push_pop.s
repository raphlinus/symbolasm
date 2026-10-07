.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global calls_out
.thumb_func
calls_out:
    push {r4, lr}
    mov r4, r0
    bl elsewhere
    mov r0, r4
    pop {r4, pc}
.section .text
.global rename
.thumb_func
rename:
    mov r2, r0
    sub r2, r2, #1
    add r2, r2, #1
    bx lr
.section .text
.global rename_in_it
.thumb_func
rename_in_it:
    cmp r0, #1
    ite eq
    moveq r0, r0
    movne r0, #2
    bx lr
.section .text
.global irqs
.thumb_func
irqs:
    cpsid i
    cpsie i
    bx lr
