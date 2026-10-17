.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global fill_16
.type fill_16, %function
.thumb_func
fill_16:
    lsrs r3, r0, #2
    itt cs
    subcs r2, r2, #1
    strhcs r1, [r0], #2
    lsrs r3, r2, #1
    beq .Lfill_16.finish
    pkhbt r1, r1, r1, lsl #16
    subs r3, r3, #2
    stm r0!, {r1}
    blo .Lfill_16.finish
    stm r0!, {r1}
    beq .Lfill_16.finish
    lsrs r3, r3, #1
    stm r0!, {r1}
    beq .Lfill_16.finish
    it cs
    stmcs r0!, {r1}
    subs r3, r3, #2
    stm r0!, {r1}
    blo .Lfill_16.finish
    stm r0!, {r1}
    stm r0!, {r1}
    beq .Lfill_16.finish
.Lfill_16_0_loop:
    subs r3, r3, #2
    stm r0!, {r1}
    stm r0!, {r1}
    blo .Lfill_16.finish
    stm r0!, {r1}
    stm r0!, {r1}
    bne .Lfill_16_0_loop
.Lfill_16_0_break:
.Lfill_16.finish:
    lsrs r2, r2, #1
    it cs
    strhcs r1, [r0]
    bx lr
.size fill_16, . - fill_16
