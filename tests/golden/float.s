.cpu cortex-m33
.syntax unified
.thumb
.fpu fpv5-sp-d16
.section .text
.global arith
.type arith, %function
.thumb_func
arith:
    vadd.f32 s2, s0, s1
    vsub.f32 s2, s2, s0
    vmul.f32 s2, s2, s1
    vdiv.f32 s2, s2, s0
    vneg.f32 s2, s2
    vmla.f32 s2, s0, s1
    vmls.f32 s2, s0, s1
    vnmul.f32 s3, s0, s1
    vfma.f32 s3, s0, s1
    vfms.f32 s3, s0, s1
    vfnma.f32 s3, s0, s1
    vfnms.f32 s3, s0, s1
    vnmla.f32 s2, s0, s1
    vnmls.f32 s2, s0, s1
    vnmls.f32 s2, s0, s1
    vmov.f32 s0, s3
    bx lr
.size arith, . - arith
.section .text
.global consts
.type consts, %function
.thumb_func
consts:
    vmov.f32 s1, #1.0
    vmov.f32 s2, #-0.125
    vldr s3, =0x3dcccccd
    vldr s4, =0x0
    vmov.f32 s5, #2.0
    vmov.f32 s6, #5.0
    vldr s7, HALF
    vmov.f32 s0, #2.5
    ldr r0, =1075838976
    bx lr
.size consts, . - consts
.section .text
.global intrinsics
.type intrinsics, %function
.thumb_func
intrinsics:
    vabs.f32 s1, s0
    vsqrt.f32 s1, s1
    vrintm.f32 s1, s0
    vrintp.f32 s1, s0
    vrinta.f32 s1, s0
    vrintn.f32 s1, s0
    vrintz.f32 s1, s0
    vminnm.f32 s2, s0, s1
    vmaxnm.f32 s0, s2, s0
    bx lr
.size intrinsics, . - intrinsics
.section .text
.global convert
.type convert, %function
.thumb_func
convert:
    vmov s0, r0
    vmov s1, r1
    vcvt.f32.s32 s2, s0
    vcvt.f32.u32 s3, s1
    vcvt.s32.f32 s4, s2
    vcvt.u32.f32 s5, s3
    vcvtm.s32.f32 s6, s2
    vcvtp.s32.f32 s6, s2
    vcvta.s32.f32 s6, s2
    vcvtn.s32.f32 s6, s2
    vcvt.s32.f32 s6, s2
    vmov r0, s4
    vmov r1, s5
    bx lr
.size convert, . - convert
.section .text
.global bits
.type bits, %function
.thumb_func
bits:
    vmov r0, s0
    eor r0, r0, #2147483648
    vmov s0, r0
    bx lr
.size bits, . - bits
.section .text
.global memory
.type memory, %function
.thumb_func
memory:
    vldr s0, [r0]
    vldr s1, [r0, #4]
    vstr s0, [r0, #8]
    vldr s2, [r1]
    vstr s2, [r1, #8]
    vldr s2, [r1, #-4]
    vldmia r1, {s4, s5}
    vstmia r1, {s4, s5}
    vldmia r1!, {s2}
    vstmia r1!, {s2}
    vldmia r1!, {s4, s5}
    vstmia r1!, {s4, s5}
    vldmia r1, {s6, s7}
    vadd.f32 s2, s6, s7
    ldr r2, =SCALE
    vldr s2, [r2, #4]
    bx lr
.size memory, . - memory
.section .text
.global compare
.type compare, %function
.thumb_func
compare:
    vcmp.f32 s0, s1
    vmrs APSR_nzcv, fpscr
    it gt
    vmovgt.f32 s0, s1
    vcmpe.f32 s0, #0
    vmrs APSR_nzcv, fpscr
    bmi .Lcompare.neg
    cmp r0, #0
    it eq
    vcmpeq.f32 s0, s1
    bx lr
.Lcompare.neg:
    vmov.f32 s0, s1
    bx lr
.size compare, . - compare
.section .text
.global round_mode
.type round_mode, %function
.thumb_func
round_mode:
    vmrs r0, fpscr
    orr r1, r0, #12582912
    vmsr fpscr, r1
    vmsr fpscr, r0
    bx lr
.size round_mode, . - round_mode
.section .text
.global saved
.type saved, %function
.thumb_func
saved:
    push {r4, lr}
    vpush {s16, s17}
    vmov.f32 s16, s0
    bl scale
    vadd.f32 s0, s0, s16
    vpop {s16, s17}
    pop {r4, pc}
.size saved, . - saved
.section .text
.global on_stack
.type on_stack, %function
.thumb_func
on_stack:
    sub sp, sp, #8
    vstr s0, [sp]
    vldr s1, [sp]
    add sp, sp, #8
    bx lr
.size on_stack, . - on_stack
.section .text
.balign 4
SCALE:
    .word 0x3fc00000
    .word 0xbe800000
    .word 0x3dcccccd
.section .text
.balign 4
HALF:
    .word 0x3f000000
