.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global main
.thumb_func
main:
    ldrsb r1, [r0]
    strh r1, [r0]
    strb r1, [r0], #8
    ldrsb r1, [r0], #-8
    asr r3, r1, #1
    add r3, r3, r1
    cmp r3, #4
loop:
    subs r1, r1, #1
    ite eq
    moveq r1, #0
    movne r1, #1
    mov r2, r1
    bne loop
    eor r1, r2, r3
    orr r1, r2, r3
    and r1, r2, r3
    lsl r1, r2, #1
    asr r1, r2, #1
    asr r1, r2, #1
    rsb r1, r2, #0
    mvn r1, r2
    mov r1, #1
    add r1, r2, r2, lsl #1
    add r1, r2, r2, asr #1
    add r1, r2, r2, asr #1
    sbfx r1, r2, #2, #3
    bfc r1, #11, #6
    bfi r1, r2, #17, #4
    bx lr
.section .text
.global tuples
.thumb_func
tuples:
    mov r1, #1
    mov r2, #2
    stm r0!, {r1, r2}
    stm r0, {r1, r2}
    strd r2, r2, [r0]
    strd r2, r2, [r0], #12
    strd r2, r2, [r0, #4]
    ldm r0, {r1, r2}
    ldm r0!, {r1, r2}
    ldrd r3, r1, [r0]
    ldm r0, {r1, r4}
    add r1, r3, r4
.section .text
.global list_len
.thumb_func
list_len:
    mov r1, #0
    cbz r0, done
loop2:
    ldr r2, [r0, #0]
    add r2, r2, #1
    str r2, [r0, #0]
    ldr r0, [r0, #4]
    add r1, r1, #1
    cmp r0, #0
    bne loop2
done:
    mov r0, r1
    bx lr
