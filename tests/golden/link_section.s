.cpu cortex-m33
.syntax unified
.thumb
.section .ram.dma_irq_0_handler,"ax",%progbits
.balign 4
.global dma_irq_0_handler
.type dma_irq_0_handler, %function
.thumb_func
dma_irq_0_handler:
    ldr r0, =dvi
    ldr r1, [r0, #4]
    eor r1, r1, #1
    str r1, [r0, #4]
    bx lr
.size dma_irq_0_handler, . - dma_irq_0_handler
.section .data.in_data
.balign 4
.global in_data
.type in_data, %function
.thumb_func
in_data:
    bx lr
.size in_data, . - in_data
.section .text
.global in_text
.type in_text, %function
.thumb_func
in_text:
    bx lr
.size in_text, . - in_text
.section .scratch_x.dvi,"aw",%progbits
.balign 4
dvi:
    .space 8
.section .bss
.balign 4
in_bss:
    .space 4
