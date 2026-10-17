.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global dead
.type dead, %function
.thumb_func
dead:
    movs r3, #5
    adds r3, r3, #1
    adds r3, r0, r1
    lsrs r3, r0, #3
    lsrs r3, r0, #24
    ands r0, r0, r1
    ands r1, r1, r0
    rsbs r0, r1, #0
    mvns r0, r1
    bx lr
.size dead, . - dead
.section .text
.global live
.type live, %function
.thumb_func
live:
.Llive_0_loop:
    subs r0, r0, #1
    mov r3, #5
    add r1, r1, #1
    mov r12, #7
    bne .Llive_0_loop
.Llive_0_break:
    cmp r0, #3
    add r1, r1, #2
    bhi .Llive.done
    cmp r0, #3
    movs r3, #1
    bcs .Llive.done
    cmp r0, #3
    it eq
    addeq r1, r1, #1
    cmp r0, #3
    movs r1, #1
    bl other
.Llive.done:
    bx lr
.size live, . - live
.section .text
.global high
.type high, %function
.thumb_func
high:
    mov r8, #5
    adds r0, r0, #1
    bx lr
.size high, . - high
