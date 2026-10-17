.section .text
.global l3_pow_43
 l3_pow_43:
    cmp r0, #129
    adr r1, l3_pow_43_tab
    bhs 2f
    vldr s0, [r1, r0, lsl #2]
    bx lr
2:
    cmp r0, #1024
    itte cc
    lslcc r0, r0, #3
    vmovcc s0, #48 // 16.0
    vldrcs s0, l3_pow_r43_c_256
    // xs = r0, mult = s0
    sbfx r3, r0, #0, #6
    subs r2, r0, r3
    vmov s1, r3
    vcvt.f32.s32 s1, s1
    vmov s2, r2
    vcvt.f32.u32 s2, s2
    vdiv.f32 s1, s1, s2 // frac = s1
    lsrs r0, r2, #6
    cmp r0, #129
    it hs
    movshs r0, #129
    add r0, r1, r0, lsl #2
    vldr s2, l3_pow_r43_2_9
    vldr s3, l3_pow_r43_4_3
    vmul s2, s2, s1 // frac * 2/9
    vldr s4, [r0]
    vadd s2, s2, s3 // 4/3 + frac * 2/9
    vmul s0, s0, s4 // G_POW[] * mult
    vmul s2, s2, s1
    vmov s1, #112 // 1.0
    vadd s1, s1, s2 // 1 + frac * (4/3 + frac * 2/9)
    vmul s0, s0, s1
    bx lr
    .balign 4
l3_pow_r43_c_256:
    .word 0x43800000
l3_pow_r43_2_9:
    .word 0x3e638e39
l3_pow_r43_4_3:
    .word 0x3faaaaab
