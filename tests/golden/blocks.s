.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global count_down
.thumb_func
count_down:
.Lcount_down_0_loop:
    subs r0, r0, #1
    bne .Lcount_down_0_loop
.Lcount_down_0_end:
    bx lr
.section .text
.global nested
.thumb_func
nested:
.Lnested_0_loop:
    mov r1, #10
.Lnested_1_loop:
    subs r1, r1, #1
    bne .Lnested_1_loop
.Lnested_1_end:
    subs r0, r0, #1
    bhs .Lnested_0_loop
.Lnested_0_end:
    bx lr
.section .text
.global choose
.thumb_func
choose:
    cmp r0, #3
.Lchoose_0_loop:
    blo .Lchoose_0_else
    mov r0, #1
    b .Lchoose_0_end
.Lchoose_0_else:
    mov r0, #2
.Lchoose_0_end:
    bx lr
.section .text
.global skip
.thumb_func
skip:
.Lskip_0_loop:
    cbz r0, .Lskip_0_end
    ldr r1, [r0]
    cbnz r1, .Lskip_0_end
    str r0, [r0]
.Lskip_0_end:
    bx lr
.section .text
.global with_it
.thumb_func
with_it:
.Lwith_it_0_loop:
    cmp r0, #5
    it hi
    movhi r0, #5
    bhi .Lwith_it_0_loop
.Lwith_it_0_end:
    bx lr
