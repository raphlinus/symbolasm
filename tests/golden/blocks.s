.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global count_down
.type count_down, %function
.thumb_func
count_down:
.Lcount_down_0_loop:
    subs r0, r0, #1
    bne .Lcount_down_0_loop
.Lcount_down_0_break:
    bx lr
.size count_down, . - count_down
.section .text
.global nested
.type nested, %function
.thumb_func
nested:
.Lnested_0_loop:
    movs r1, #10
.Lnested_1_loop:
    subs r1, r1, #1
    bne .Lnested_1_loop
.Lnested_1_break:
    subs r0, r0, #1
    bhs .Lnested_0_loop
.Lnested_0_break:
    bx lr
.size nested, . - nested
.section .text
.global choose
.type choose, %function
.thumb_func
choose:
    cmp r0, #3
    blo .Lchoose_0_else
    movs r0, #1
    b .Lchoose_0_end
.Lchoose_0_else:
    movs r0, #2
.Lchoose_0_end:
    bx lr
.size choose, . - choose
.section .text
.global fall_into_else
.type fall_into_else, %function
.thumb_func
fall_into_else:
    cmp r0, #3
    blo .Lfall_into_else_0_end
    movs r0, #1
    cmp r0, #2
    beq .Lfall_into_else_0_end
.Lfall_into_else_0_else:
    movs r0, #2
.Lfall_into_else_0_end:
    bx lr
.size fall_into_else, . - fall_into_else
.section .text
.global skip
.type skip, %function
.thumb_func
skip:
    cbz r0, .Lskip_0_end
    ldr r1, [r0]
    cbnz r1, .Lskip_0_end
    str r0, [r0]
.Lskip_0_end:
    bx lr
.size skip, . - skip
.section .text
.global with_it
.type with_it, %function
.thumb_func
with_it:
.Lwith_it_0_loop:
    cmp r0, #5
    it hi
    movhi r0, #5
    bhi .Lwith_it_0_loop
.Lwith_it_0_break:
    bx lr
.size with_it, . - with_it
.section .text
.global loop_in_if
.type loop_in_if, %function
.thumb_func
loop_in_if:
.Lloop_in_if_0_loop:
    ldm r0!, {r2}
    cbz r2, .Lloop_in_if_1_end
    subs r1, r1, #1
    bne .Lloop_in_if_0_loop
    b .Lloop_in_if_0_break
.Lloop_in_if_1_end:
    subs r1, r1, #1
    bne .Lloop_in_if_0_loop
.Lloop_in_if_0_break:
    bx lr
.size loop_in_if, . - loop_in_if
