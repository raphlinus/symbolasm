.cpu cortex-m33
.syntax unified
.thumb
.section .text
.global copy_data
.thumb_func
copy_data:
    ldr r0, =_data_source
    ldr r1, =_data_start
    movw r2, #:lower16:_data_size_words
data_loop:
    subs r2, r2, #1
    ldr r3, [r0], #4
    str r3, [r1], #4
    bne data_loop
    ldr r1, =_data_source
    bx lr
.section .text
.global halves
.thumb_func
halves:
    movw r0, #:lower16:_data_start
    movt r0, #:upper16:_data_start
    mov r1, #22136
    movt r1, #4660
    bx lr
