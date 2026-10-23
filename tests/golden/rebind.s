.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global read_after_call
.type read_after_call, %function
.thumb_func
read_after_call:
    push {r4, lr}
    mov r4, r0
    bl other
    ldrb r0, [r4, #1]
    pop {r4, pc}
.size read_after_call, . - read_after_call
.section .text
.global write_after_call
.type write_after_call, %function
.thumb_func
write_after_call:
    push {r4, lr}
    mov r4, r0
    bl other
    adds r4, r4, #1
    ldrb r0, [r4, #0]
    pop {r4, pc}
.size write_after_call, . - write_after_call
.section .text
.global old_reg_reused
.type old_reg_reused, %function
.thumb_func
old_reg_reused:
    push {r4, lr}
    mov r4, r0
    movs r0, #5
    bl other
    ldrb r0, [r4, #1]
    pop {r4, pc}
.size old_reg_reused, . - old_reg_reused
