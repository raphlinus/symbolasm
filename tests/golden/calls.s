.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global divmod
.type divmod, %function
.thumb_func
divmod:
    udiv r2, r0, r1
    mls r1, r2, r1, r0
    mov r0, r2
    bx lr
.size divmod, . - divmod
.section .text
.global print_digits
.type print_digits, %function
.thumb_func
print_digits:
    push {r3, r4, r5, r6, r7, lr}
.Lprint_digits_0_while:
    cbz r0, .Lprint_digits_0_break
.Lprint_digits_0_loop:
    movs r1, #10
    bl divmod
    mov r4, r0
    add r0, r1, #48
    bl putc
    mov r0, r4
    cmp r0, #0
    bne .Lprint_digits_0_loop
.Lprint_digits_0_break:
    pop {r3, r4, r5, r6, r7, pc}
.size print_digits, . - print_digits
.section .text
.global draw_one
.type draw_one, %function
.thumb_func
draw_one:
    push {lr}
    ldr r4, [r0, #0]
    blx r4
    pop {pc}
.size draw_one, . - draw_one
.section .text
.global five
.type five, %function
.thumb_func
five:
    push {r4, lr}
    sub sp, sp, #4
    movs r4, #5
    push {r4}
    bl sum5
    add sp, sp, #8
    pop {r4, pc}
.size five, . - five
.section .text
.global divmod10
.type divmod10, %function
.thumb_func
divmod10:
    movs r1, #10
    b divmod
.size divmod10, . - divmod10
