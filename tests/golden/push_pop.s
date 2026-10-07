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
