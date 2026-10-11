.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global wait
.type wait, %function
.thumb_func
wait:
.Lwait_0_loop:
    ldr r1, [r0]
    cbnz r1, .Lwait_0_end
    wfe
    b .Lwait_0_loop
.Lwait_0_end:
    dmb
    dsb
    isb
    sev
    wfi
    nop
    cmp r1, #1
    it eq
    wfeeq
    bx lr
.size wait, . - wait
.section .text
.global vars
.type vars, %function
.thumb_func
vars:
    adds r0, r0, #1
    mov r1, r0
    bx lr
.size vars, . - vars
