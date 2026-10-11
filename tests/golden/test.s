.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global main
.type main, %function
.thumb_func
main:
    ldrsb r1, [r0]
    strh r1, [r0]
    strb r1, [r0], #8
    ldrsb r1, [r0], #-8
    asrs r3, r1, #1
    adds r3, r3, r1
    cmp r3, #4
.Lmain.loop:
    subs r1, r1, #1
    ite eq
    moveq r1, #0
    movne r1, #1
    mov r2, r1
    bne .Lmain.loop
    eor r1, r2, r3
    orr r1, r2, r3
    and r1, r2, r3
    lsls r1, r2, #1
    asrs r1, r2, #1
    asrs r1, r2, #1
    rsbs r1, r2, #0
    mvns r1, r2
    movs r1, #1
    add r1, r2, r2, lsl #1
    add r1, r2, r2, asr #1
    add r1, r2, r2, asr #1
    sbfx r1, r2, #2, #3
    bfc r1, #11, #6
    bfi r1, r2, #17, #4
    bx lr
.size main, . - main
.section .text
.global tuples
.type tuples, %function
.thumb_func
tuples:
    movs r1, #1
    movs r2, #2
    stm r0!, {r1, r2}
    stm r0, {r1, r2}
    strd r2, r2, [r0]
    strd r2, r2, [r0], #12
    strd r2, r2, [r0, #4]
    ldm r0, {r1, r2}
    ldm r0!, {r1, r2}
    ldrd r3, r1, [r0]
    ldm r0, {r1, r4}
    adds r1, r3, r4
.size tuples, . - tuples
.section .text
.global list_len
.type list_len, %function
.thumb_func
list_len:
    movs r1, #0
    cbz r0, .Llist_len.done
.Llist_len.loop2:
    ldr r2, [r0, #0]
    adds r2, r2, #1
    str r2, [r0, #0]
    ldr r0, [r0, #4]
    adds r1, r1, #1
    cmp r0, #0
    bne .Llist_len.loop2
.Llist_len.done:
    mov r0, r1
    bx lr
.size list_len, . - list_len
