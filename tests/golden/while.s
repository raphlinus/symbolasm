.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global sum
.type sum, %function
.thumb_func
sum:
    movs r2, #0
.Lsum_0_while:
    cbz r1, .Lsum_0_break
.Lsum_0_loop:
    ldm r0!, {r3}
    adds r2, r2, r3
    subs r1, r1, #1
    bne .Lsum_0_loop
.Lsum_0_break:
    mov r0, r2
    bx lr
.size sum, . - sum
.section .text
.global skip_zeros
.type skip_zeros, %function
.thumb_func
skip_zeros:
.Lskip_zeros_0_while:
    cmp r0, r1
    beq .Lskip_zeros_0_break
.Lskip_zeros_0_loop:
    ldm r0!, {r2}
    cmp r2, #1
    blo .Lskip_zeros_0_while
    str r2, [r0]
    b .Lskip_zeros_0_while
.Lskip_zeros_0_break:
    bx lr
.size skip_zeros, . - skip_zeros
.section .text
.global nested
.type nested, %function
.thumb_func
nested:
.Lnested_0_while:
    cbz r1, .Lnested_0_break
.Lnested_0_loop:
    ldm r0!, {r2}
    cbz r2, .Lnested_1_end
.Lnested_2_loop:
    subs r2, r2, #1
    bne .Lnested_2_loop
.Lnested_2_break:
    b .Lnested_0_while
.Lnested_1_end:
    subs r1, r1, #1
    bne .Lnested_0_loop
.Lnested_0_break:
    bx lr
.size nested, . - nested
