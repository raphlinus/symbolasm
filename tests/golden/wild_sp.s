.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global maybe_push
.type maybe_push, %function
.thumb_func
maybe_push:
    cbz r0, .Lmaybe_push.skip
    push {r4}
.Lmaybe_push.skip:
    bx lr
.size maybe_push, . - maybe_push
.section .text
.global push_n
.type push_n, %function
.thumb_func
push_n:
.Lpush_n_0_loop:
    push {r0}
    subs r0, r0, #1
    bne .Lpush_n_0_loop
.Lpush_n_0_break:
    bx lr
.size push_n, . - push_n
