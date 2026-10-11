.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global f
.type f, %function
.thumb_func
f:
    cbz r0, .Lf.done
    movs r0, #1
.Lf.done:
    bx lr
.size f, . - f
.section .text
.global g
.type g, %function
.thumb_func
g:
    cbz r0, .Lg.done
    b f
.Lg.done:
    bx lr
.size g, . - g
