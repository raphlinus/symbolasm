.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global calls_out
.type calls_out, %function
.thumb_func
calls_out:
    push {r4, lr}
    mov r4, r0
    bl elsewhere
    mov r0, r4
    pop {r4, pc}
.size calls_out, . - calls_out
.section .text
.global rename
.type rename, %function
.thumb_func
rename:
    mov r2, r0
    subs r2, r2, #1
    adds r2, r2, #1
    bx lr
.size rename, . - rename
.section .text
.global rename_in_it
.type rename_in_it, %function
.thumb_func
rename_in_it:
    cmp r0, #1
    ite eq
    moveq r0, r0
    movne r0, #2
    bx lr
.size rename_in_it, . - rename_in_it
.section .text
.global irqs
.type irqs, %function
.thumb_func
irqs:
    cpsid i
    cpsie i
    bx lr
.size irqs, . - irqs
.section .text
.global spill
.type spill, %function
.thumb_func
spill:
    push {r0, lr}
    bl elsewhere
    pop {r1, lr}
    str r0, [r1]
    bx lr
.size spill, . - spill
