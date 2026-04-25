.section .text
.global _start
 _start:
    mov r0, #0x21
    bkpt 1
    bkpt 0
