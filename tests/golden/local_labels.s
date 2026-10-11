.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global f
.thumb_func
f:
    cbz r0, .Lf.done
    mov r0, #1
.Lf.done:
    bx lr
.section .text
.global g
.thumb_func
g:
    cbz r0, .Lg.done
    b f
.Lg.done:
    bx lr
