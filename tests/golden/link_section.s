.cpu cortex-m33
.syntax unified
.thumb
.section .ram.dma_irq_0_handler,"ax",%progbits
.balign 4
.global dma_irq_0_handler
.thumb_func
dma_irq_0_handler:
    ldr r0, =dvi
    ldr r1, [r0, #4]
    eor r1, r1, #1
    str r1, [r0, #4]
    bx lr
.section .data.in_data
.balign 4
.global in_data
.thumb_func
in_data:
    bx lr
.section .text
.global in_text
.thumb_func
in_text:
    bx lr
.section .scratch_x.dvi,"aw",%progbits
.balign 4
dvi:
    .space 8
.section .bss
.balign 4
in_bss:
    .space 4
