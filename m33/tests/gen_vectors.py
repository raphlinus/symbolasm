# Copyright 2026 Raph Levien
# SPDX-License-Identifier: Apache-2.0 OR MIT

"""Generate instruction test vectors for m33, using unicorn as the reference.

Each case is a short instruction sequence, run from random register and flag
values on unicorn's Cortex-M33 model. The vectors record the starting state,
the final registers and flags, and every memory write. tests/instrs.rs runs
the same cases on m33 and compares.

Memory is 64 KB filled with a fixed pattern (see pattern()), with the code at
0x8000. In "mem" cases the registers are word aligned addresses in
0x400..0x1400, so loads and stores land in the data area below the code.

The FPU cases, in tests/fp_vectors.txt, also have the starting and final
s0-s31 and FPSCR, in two more fields after the core registers.

Needs arm-none-eabi-as and `pip install unicorn`. Run from the m33
directory: python3 tests/gen_vectors.py > tests/vectors.txt, and
python3 tests/gen_vectors.py fp > tests/fp_vectors.txt
"""

import os
import random
import struct
import subprocess
import sys
import tempfile

from unicorn import UC_ARCH_ARM, UC_MODE_MCLASS, UC_MODE_THUMB, UC_HOOK_MEM_WRITE, Uc
from unicorn import arm_const as A

MEM_SIZE = 0x10000
CODE = 0x8000
CASES = 6

# (asm, mode). Instructions are separated by ';'. Modes: "alu" random
# values, "shift" small values in some registers so register shifts are in
# range, "mem" addresses, "tbl" a table base and small index, "branch" a
# Thumb code address in r3.
TEMPLATES = [
    # 16 bit data processing
    ("adds r0, r1, r2", "alu"),
    ("subs r0, r1, #5", "alu"),
    ("adds r3, #200", "alu"),
    ("subs r3, #200", "alu"),
    ("movs r4, #77", "alu"),
    ("cmp r5, #10", "alu"),
    ("cmp r5, r6", "alu"),
    ("cmp r8, r9", "alu"),
    ("cmn r5, r6", "alu"),
    ("ands r0, r1", "alu"),
    ("eors r0, r1", "alu"),
    ("orrs r0, r1", "alu"),
    ("bics r0, r1", "alu"),
    ("mvns r0, r1", "alu"),
    ("tst r0, r1", "alu"),
    ("adcs r0, r1", "alu"),
    ("sbcs r0, r1", "alu"),
    ("rsbs r0, r1, #0", "alu"),
    ("muls r0, r1, r0", "alu"),
    ("lsls r0, r1, #3", "alu"),
    ("lsrs r0, r1, #31", "alu"),
    ("lsrs r0, r1, #32", "alu"),
    ("asrs r0, r1, #7", "alu"),
    ("lsls r0, r1", "shift"),
    ("lsrs r0, r1", "shift"),
    ("asrs r0, r1", "shift"),
    ("rors r0, r1", "shift"),
    ("add r0, r9", "alu"),
    ("mov r10, r3", "alu"),
    ("sxth r0, r1", "alu"),
    ("sxtb r0, r1", "alu"),
    ("uxth r0, r1", "alu"),
    ("uxtb r0, r1", "alu"),
    ("rev r0, r1", "alu"),
    ("rev16 r0, r1", "alu"),
    ("revsh r0, r1", "alu"),
    ("add r0, sp, #16", "mem"),
    ("add sp, #16", "mem"),
    ("sub sp, #16", "mem"),
    ("adr r0, .+8", "alu"),
    # 32 bit data processing, modified immediate
    ("add r0, r1, #0x100", "alu"),
    ("adds r0, r1, #0xff00", "alu"),
    ("sub r0, r1, #0x1f0", "alu"),
    ("subs r8, r9, #0x80000000", "alu"),
    ("and r0, r1, #0xff", "alu"),
    ("ands r0, r1, #0xf0000000", "alu"),
    ("bic r0, r1, #0x3c", "alu"),
    ("orr r0, r1, #0x00ff00ff", "alu"),
    ("orn r0, r1, #0xff", "alu"),
    ("eors r0, r1, #0x80000000", "alu"),
    ("mov r8, #0x12000000", "alu"),
    ("movs r8, #0x80000000", "alu"),
    ("mvn r8, #0xff", "alu"),
    ("adc r0, r1, #3", "alu"),
    ("sbcs r0, r1, #3", "alu"),
    ("rsb r0, r1, #100", "alu"),
    ("rsbs r0, r1, #0", "alu"),
    ("tst r1, #0x80000000", "alu"),
    ("teq r1, #0x400", "alu"),
    ("cmp r9, #0x100", "alu"),
    ("cmn r9, #0x100", "alu"),
    # 32 bit plain binary immediate
    ("addw r0, r1, #4095", "alu"),
    ("subw r0, r1, #1234", "alu"),
    ("movw r9, #0xbeef", "alu"),
    ("movt r9, #0xdead", "alu"),
    ("adr.w r0, .+1000", "alu"),
    ("adr.w r0, .-100", "alu"),
    ("ubfx r0, r1, #8, #8", "alu"),
    ("ubfx r0, r1, #0, #32", "alu"),
    ("sbfx r0, r1, #4, #12", "alu"),
    ("bfi r0, r1, #8, #4", "alu"),
    ("bfc r0, #4, #20", "alu"),
    ("usat r0, #8, r1", "alu"),
    ("ssat r0, #16, r1", "alu"),
    ("usat r0, #5, r1, lsl #3", "alu"),
    # 32 bit data processing, shifted register
    ("add r0, r1, r2, lsl #2", "alu"),
    ("adds r0, r1, r2, lsr #3", "alu"),
    ("sub r0, r1, r2, asr #31", "alu"),
    ("subs r0, r1, r2, ror #8", "alu"),
    ("and r0, r1, r2, lsl #1", "alu"),
    ("ands r0, r1, r2, lsr #1", "alu"),
    ("orrs.w r0, r1, r2", "alu"),
    ("orr r0, r1, r2, lsl #16", "alu"),
    ("eor r0, r1, r2, ror #16", "alu"),
    ("bics r0, r1, r2, asr #2", "alu"),
    ("orn r0, r1, r2", "alu"),
    ("mvns r0, r2, lsl #4", "alu"),
    ("mov.w r0, r2, lsl #4", "alu"),
    ("movs.w r8, r2, lsr #1", "alu"),
    ("rrx r0, r2", "alu"),
    ("rrxs r0, r2", "alu"),
    ("adc r0, r1, r2", "alu"),
    ("sbcs.w r0, r1, r2", "alu"),
    ("rsb r0, r1, r2, lsl #1", "alu"),
    ("cmp.w r1, r2, lsl #4", "alu"),
    ("cmn.w r1, r2", "alu"),
    ("tst.w r1, r2, lsr #2", "alu"),
    ("teq r1, r2", "alu"),
    ("pkhbt r0, r1, r2, lsl #16", "alu"),
    ("pkhtb r0, r1, r2, asr #16", "alu"),
    ("pkhbt r0, r1, r2", "alu"),
    # register shifts
    ("lsl.w r0, r1, r2", "shift"),
    ("lsr.w r0, r1, r2", "shift"),
    ("asrs.w r0, r1, r2", "shift"),
    ("ror.w r0, r1, r2", "shift"),
    ("lsls.w r8, r1, r2", "shift"),
    # extend, extend and add
    ("uxtb.w r8, r1", "alu"),
    ("uxtb.w r0, r1, ror #8", "alu"),
    ("uxth.w r8, r1", "alu"),
    ("uxth.w r0, r1, ror #16", "alu"),
    ("sxtb.w r8, r1", "alu"),
    ("sxth.w r0, r1, ror #8", "alu"),
    ("uxtab r0, r1, r2", "alu"),
    ("uxtah r0, r1, r2, ror #8", "alu"),
    ("sxtab r0, r1, r2", "alu"),
    ("sxtah r0, r1, r2", "alu"),
    # other three register
    ("rev.w r8, r1", "alu"),
    ("rev16.w r8, r1", "alu"),
    ("revsh.w r8, r1", "alu"),
    ("rbit r0, r1", "alu"),
    ("clz r0, r1", "alu"),
    # multiply and divide
    ("mul r8, r1, r2", "alu"),
    ("mla r0, r1, r2, r3", "alu"),
    ("mls r0, r1, r2, r3", "alu"),
    ("umull r0, r1, r2, r3", "alu"),
    ("smull r0, r1, r2, r3", "alu"),
    ("umlal r0, r1, r2, r3", "alu"),
    ("smlal r0, r1, r2, r3", "alu"),
    ("udiv r0, r1, r2", "alu"),
    ("sdiv r0, r1, r2", "alu"),
    ("udiv r0, r1, r2", "shift"),
    ("sdiv r0, r1, r2", "shift"),
    # 16 bit loads and stores
    ("ldr r0, [r1, #4]", "mem"),
    ("str r0, [r1, #124]", "mem"),
    ("ldrb r0, [r1, #31]", "mem"),
    ("strb r0, [r1, #3]", "mem"),
    ("ldrh r0, [r1, #62]", "mem"),
    ("strh r0, [r1, #2]", "mem"),
    ("ldr r0, [r1, r2]", "mem"),
    ("str r0, [r1, r2]", "mem"),
    ("ldrsb r0, [r1, r2]", "mem"),
    ("ldrsh r0, [r1, r2]", "mem"),
    ("ldrb r0, [r1, r2]", "mem"),
    ("strh r0, [r1, r2]", "mem"),
    ("ldr r0, [sp, #8]", "mem"),
    ("str r0, [sp, #1020]", "mem"),
    ("ldr r0, .+12", "mem"),
    ("ldm r1!, {r0, r2, r3}", "mem"),
    ("ldm r1, {r0, r1, r2}", "mem"),
    ("stm r1!, {r2, r3, r4}", "mem"),
    ("push {r0, r4, lr}", "mem"),
    ("pop {r0, r4}", "mem"),
    # 32 bit loads and stores
    ("ldr.w r8, [r1, #4000]", "mem"),
    ("str.w r8, [r1, #2048]", "mem"),
    ("ldr r0, [r1, #-200]", "mem"),
    ("str r0, [r1, #-4]", "mem"),
    ("ldr r0, [r1, #8]!", "mem"),
    ("ldr r0, [r1, #-8]!", "mem"),
    ("str r0, [r1], #4", "mem"),
    ("ldr r0, [r1], #-12", "mem"),
    ("ldrb.w r0, [r1, #1001]", "mem"),
    ("ldrsb.w r0, [r1, #3]", "mem"),
    ("ldrh.w r0, [r1, #1002]", "mem"),
    ("ldrsh.w r0, [r1, #-2]", "mem"),
    ("strb.w r9, [r1, #-1]", "mem"),
    ("strh r0, [r1], #2", "mem"),
    ("ldrb r0, [r1], #1", "mem"),
    ("ldr.w r0, [r1, r2, lsl #2]", "mem"),
    ("str.w r0, [r1, r2, lsl #1]", "mem"),
    ("ldrh.w r0, [r1, r2, lsl #1]", "mem"),
    ("ldr.w r0, .+100", "mem"),
    ("ldr.w r0, .-100", "mem"),
    ("ldrd r0, r2, [r1, #8]", "mem"),
    ("ldrd r0, r2, [r1], #8", "mem"),
    ("ldrd r0, r2, [r1, #-8]!", "mem"),
    ("strd r0, r2, [r1, #16]", "mem"),
    ("strd r0, r2, [r1], #-8", "mem"),
    ("ldm.w r1!, {r0, r2, r8, r9}", "mem"),
    ("stm.w r1!, {r0, r2, r8, r9}", "mem"),
    ("ldmdb r1!, {r0, r2}", "mem"),
    ("stmdb r1!, {r0, r2}", "mem"),
    ("push.w {r4, r5, r8, lr}", "mem"),
    ("pop.w {r4, r5, r8}", "mem"),
    ("ldrex r0, [r1]", "mem"),
    ("ldrex r0, [r1, #8]", "mem"),
    ("ldrex r0, [r1]; strex r2, r3, [r1]", "mem"),
    ("strex r2, r3, [r1]", "mem"),
    ("lda r0, [r1]", "mem"),
    ("ldaex r0, [r1]", "mem"),
    ("stl r0, [r1]", "mem"),
    ("ldab r0, [r1]", "mem"),
    ("stlh r0, [r1]", "mem"),
    # branches
    ("b .+100", "alu"),
    ("b.w .+4000", "alu"),
    ("b.w .-2000", "alu"),
    ("beq .+20", "alu"),
    ("bne .-20", "alu"),
    ("bhi.w .+3000", "alu"),
    ("blt.w .-3000", "alu"),
    ("bl .+5000", "alu"),
    ("bl .-5000", "alu"),
    ("blx r3", "branch"),
    ("bx r3", "branch"),
    ("cbz r0, .+40", "shift"),
    ("cbnz r0, .+40", "shift"),
    ("tbb [r1, r2]", "tbl"),
    ("tbh [r1, r2, lsl #1]", "tbl"),
    # IT blocks
    ("it eq; moveq r0, r1", "alu"),
    ("ite hi; addhi r0, r1; subls r0, r1", "alu"),
    ("itt ne; addne r0, r1, r2; movne r3, #5", "alu"),
    ("it cs; addcs r0, r1", "alu"),
    ("itte lt; ldrlt r0, [r1]; strlt r0, [r2]; ldrge.w r0, [r1, #8]", "mem"),
    # hints and barriers
    ("nop", "alu"),
    ("dmb", "alu"),
    ("dsb", "alu"),
    ("isb", "alu"),
    ("sev", "alu"),
    ("nop.w", "alu"),
]

# FPU cases. Modes: "fp" random floats, "fprm" the same with a random
# rounding mode in FPSCR, "fpmem" floats with addresses in the core
# registers, "fpcore" floats with random core registers.
FP_TEMPLATES = [
    # arithmetic
    ("vadd.f32 s0, s1, s2", "fp"),
    ("vsub.f32 s3, s4, s5", "fp"),
    ("vmul.f32 s0, s1, s2", "fp"),
    ("vnmul.f32 s0, s1, s2", "fp"),
    ("vdiv.f32 s0, s1, s2", "fp"),
    ("vadd.f32 s31, s30, s17", "fp"),
    ("vmla.f32 s0, s1, s2", "fp"),
    ("vmls.f32 s0, s1, s2", "fp"),
    ("vnmla.f32 s0, s1, s2", "fp"),
    ("vnmls.f32 s0, s1, s2", "fp"),
    ("vfma.f32 s0, s1, s2", "fp"),
    ("vfms.f32 s0, s1, s2", "fp"),
    ("vfnma.f32 s0, s1, s2", "fp"),
    ("vfnms.f32 s0, s1, s2", "fp"),
    ("vfma.f32 s0, s0, s0", "fp"),
    # one operand
    ("vmov.f32 s0, s1", "fp"),
    ("vabs.f32 s0, s1", "fp"),
    ("vneg.f32 s0, s1", "fp"),
    ("vsqrt.f32 s0, s1", "fp"),
    ("vmov.f32 s0, #1.0", "fp"),
    ("vmov.f32 s9, #-0.125", "fp"),
    ("vmov.f32 s0, #31.0", "fp"),
    ("vmov.f32 s0, #0.1875", "fp"),
    # rounding and conversions
    ("vrintz.f32 s0, s1", "fp"),
    ("vrintr.f32 s0, s1", "fprm"),
    ("vrintx.f32 s0, s1", "fprm"),
    ("vrinta.f32 s0, s1", "fprm"),
    ("vrintn.f32 s0, s1", "fp"),
    ("vrintp.f32 s0, s1", "fp"),
    ("vrintm.f32 s0, s1", "fprm"),
    ("vcvt.f32.s32 s0, s1", "fprm"),
    ("vcvt.f32.u32 s0, s1", "fprm"),
    ("vcvt.s32.f32 s0, s1", "fprm"),
    ("vcvt.u32.f32 s0, s1", "fp"),
    ("vcvtr.s32.f32 s0, s1", "fprm"),
    ("vcvtr.u32.f32 s0, s1", "fprm"),
    ("vcvta.s32.f32 s0, s1", "fp"),
    ("vcvtn.u32.f32 s0, s1", "fp"),
    ("vcvtp.s32.f32 s0, s1", "fp"),
    ("vcvtm.u32.f32 s0, s1", "fp"),
    ("vmaxnm.f32 s0, s1, s2", "fp"),
    ("vminnm.f32 s0, s1, s2", "fp"),
    ("vseleq.f32 s0, s1, s2", "fp"),
    ("vselvs.f32 s0, s1, s2", "fp"),
    ("vselge.f32 s0, s1, s2", "fp"),
    ("vselgt.f32 s0, s1, s2", "fp"),
    # compares and FPSCR
    ("vcmp.f32 s1, s2", "fp"),
    ("vcmpe.f32 s1, s2", "fp"),
    ("vcmp.f32 s1, #0", "fp"),
    ("vcmp.f32 s1, s2; vmrs APSR_nzcv, fpscr", "fp"),
    ("vmrs r0, fpscr", "fpcore"),
    ("vmsr fpscr, r0", "fpcore"),
    # moves to and from core registers
    ("vmov s0, r1", "fpcore"),
    ("vmov r0, s1", "fpcore"),
    ("vmov s17, r9", "fpcore"),
    ("vmov s0, s1, r1, r2", "fpcore"),
    ("vmov r0, r1, s4, s5", "fpcore"),
    # loads and stores
    ("vldr s0, [r1, #8]", "fpmem"),
    ("vldr s17, [r1, #-1020]", "fpmem"),
    ("vstr s1, [r1, #1020]", "fpmem"),
    ("vstr s31, [r1, #-4]", "fpmem"),
    ("vldr s0, [sp, #12]", "fpmem"),
    ("vldr s0, .+12", "fpmem"),
    ("nop; vldr s0, .+10", "fpmem"),
    ("vldmia r1, {s0-s3}", "fpmem"),
    ("vldmia r1!, {s4-s6}", "fpmem"),
    ("vstmia r1!, {s1-s2}", "fpmem"),
    ("vstmdb r1!, {s8-s15}", "fpmem"),
    ("vldmdb r1!, {s0-s1}", "fpmem"),
    ("vpush {s16-s19}", "fpmem"),
    ("vpop {s16-s17}", "fpmem"),
    # in an IT block
    ("itte gt; vmovgt.f32 s0, s1; vaddgt.f32 s2, s3, s4; vmovle s0, r1", "fpcore"),
]

# f32 bits: zeros, ones, ties for rounding, infinities, NaNs (quiet,
# with payloads, and signaling), denormals, extremes, integers past 2^24
FP_EDGES = [
    0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x3FC00000, 0xC0200000,
    0x40200000, 0x40600000, 0xBFC00000, 0x3F000000, 0xBF000000, 0x3EFFFFFF,
    0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00001, 0x7FC12345, 0x7F800001,
    0xFFA00000, 0x00000001, 0x807FFFFF, 0x00800000, 0x7F7FFFFF, 0xFF7FFFFF,
    0x4F000000, 0x4F800000, 0xCF000000, 0xCF000001, 0x4B800001, 0x4EFFFFFF,
]

EDGES = [0, 1, 2, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF, 0xFFFFFFFE, 0x8000, 0xFFFF]


def pattern():
    """The memory fill, a xorshift32 sequence; tests/instrs.rs matches it."""
    x = 0x12345678
    out = bytearray(MEM_SIZE)
    for i in range(MEM_SIZE):
        x ^= (x << 13) & 0xFFFFFFFF
        x ^= x >> 17
        x ^= (x << 5) & 0xFFFFFFFF
        out[i] = x & 0xFF
    return bytes(out)


def assemble(asm):
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "t.s")
        obj = os.path.join(d, "t.o")
        binf = os.path.join(d, "t.bin")
        lines = [s.strip() for s in asm.split(";")]
        with open(src, "w") as f:
            f.write(".syntax unified\n.thumb\n.fpu fpv5-sp-d16\n")
            f.write("".join(line + "\n" for line in lines))
        subprocess.run(
            ["arm-none-eabi-as", "-mcpu=cortex-m33", "-o", obj, src], check=True
        )
        subprocess.run(
            ["arm-none-eabi-objcopy", "-O", "binary", "-j", ".text", obj, binf],
            check=True,
        )
        with open(binf, "rb") as f:
            return f.read(), len(lines)


def rand_regs(rng, mode):
    regs = []
    for i in range(13):
        if mode in ("mem", "tbl"):
            v = 0x400 + rng.randrange(0x400) * 4
        elif mode == "shift" and i in (0, 2):
            v = rng.choice([0, 1, 7, 31, 32, 33, 255, 256 + 3, rng.randrange(40)])
        elif rng.random() < 0.3:
            v = rng.choice(EDGES)
        else:
            v = rng.getrandbits(32)
        regs.append(v)
    if mode == "tbl":
        regs[2] = rng.randrange(16)
    if mode == "branch":
        regs[3] = (CODE + rng.randrange(0x400) * 2) | 1
    sp = 0x2000 + rng.randrange(0x100) * 4
    lr = rng.getrandbits(32) | 1
    flags = rng.randrange(16) << 28
    return regs + [sp, lr], flags


def rand_float(rng):
    r = rng.random()
    if r < 0.4:
        return rng.choice(FP_EDGES)
    if r < 0.6:
        # small values, including integers and halves
        x = rng.randrange(-40, 40) / rng.choice([1, 2, 4, 3])
        return struct.unpack("<I", struct.pack("<f", x))[0]
    if r < 0.7:
        # integers, for conversions to f32
        return rng.choice([rng.getrandbits(32), rng.randrange(-100, 100) & 0xFFFFFFFF])
    # random finite values of moderate size, so sums and products are
    # interesting
    sign = rng.getrandbits(1) << 31
    exp = rng.randrange(100, 156) << 23
    return sign | exp | rng.getrandbits(23)


def rand_fp(rng, mode):
    s = [rand_float(rng) for _ in range(32)]
    # Some cases with operands equal, for compares and max and min
    if rng.random() < 0.15:
        s[2] = s[1]
    fpscr = rng.randrange(16) << 28
    if rng.random() < 0.3:
        fpscr |= 1 << 25  # DN
    if mode == "fprm":
        fpscr |= rng.randrange(4) << 22
    return s, fpscr


def is_branch(asm):
    op = asm.split()[0]
    return op[0] == "b" and op[:3] not in ("bic", "bfi", "bfc") or op[:2] in ("cb", "tb")


def run_case(asm, code, regs, flags, mem, fp=None):
    uc = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    uc.ctl_set_cpu_model(A.UC_CPU_ARM_CORTEX_M33)
    # Room for table branches to land past the compared memory
    uc.mem_map(0, 4 * MEM_SIZE)
    uc.mem_write(0, mem)
    uc.mem_write(CODE, code)
    names = [getattr(A, f"UC_ARM_REG_R{i}") for i in range(13)]
    names += [A.UC_ARM_REG_SP, A.UC_ARM_REG_LR]
    for r, v in zip(names, regs):
        uc.reg_write(r, v)
    uc.reg_write(A.UC_ARM_REG_XPSR, flags | (1 << 24))
    if fp is not None:
        # The first FPU instruction with CONTROL.FPCA clear loads FPSCR
        # from FPDSCR, so run one (vmov s0, s0) before setting FPSCR
        stub = 0x20000
        uc.mem_write(stub, bytes.fromhex("b0ee400a"))
        uc.emu_start(stub | 1, stub + 4)
        for i, v in enumerate(fp[0]):
            uc.reg_write(A.UC_ARM_REG_S0 + i, v)
        uc.reg_write(A.UC_ARM_REG_FPSCR, fp[1])
    writes = []

    def hook(uc, access, addr, size, value, data):
        writes.append((addr, size, value & ((1 << (8 * size)) - 1)))

    uc.hook_add(UC_HOOK_MEM_WRITE, hook)
    if is_branch(asm):
        uc.emu_start(CODE | 1, 0xFFFFFFFF, count=1)
    else:
        # IT doesn't count as an instruction here, so run to the end
        uc.emu_start(CODE | 1, CODE + len(code))
    final = [uc.reg_read(r) for r in names] + [uc.reg_read(A.UC_ARM_REG_PC)]
    nzcv = uc.reg_read(A.UC_ARM_REG_XPSR) & 0xF0000000
    if fp is not None:
        s = [uc.reg_read(A.UC_ARM_REG_S0 + i) for i in range(32)]
        # The cumulative exception flags aren't compared
        fpscr = uc.reg_read(A.UC_ARM_REG_FPSCR) & ~0x9F
        return final, nzcv, writes, s + [fpscr]
    return final, nzcv, writes


def main():
    if sys.argv[1:] == ["fp"]:
        return main_fp()
    rng = random.Random(1)
    mem = pattern()
    print("# Generated by tests/gen_vectors.py; don't edit.")
    print("# asm | code | steps | r0-r12 sp lr nzcv | r0-r12 sp lr pc nzcv | writes")
    for asm, mode in TEMPLATES:
        code, n = assemble(asm)
        for _ in range(CASES):
            regs, flags = rand_regs(rng, mode)
            # Skip divides by zero and register shifts the hardware treats
            # the same as m33 anyway; unicorn's divide by zero traps.
            if "div" in asm and regs[2] == 0:
                regs[2] = 3
            final, nzcv, writes = run_case(asm, code, regs, flags, mem)
            fields = [
                asm,
                code.hex(),
                str(n),
                " ".join(f"{v:x}" for v in regs + [flags]),
                " ".join(f"{v:x}" for v in final + [nzcv]),
                " ".join(f"{a:x}:{s}:{v:x}" for a, s, v in writes),
            ]
            print(" | ".join(fields))


def main_fp():
    rng = random.Random(2)
    mem = pattern()
    print("# Generated by tests/gen_vectors.py fp; don't edit.")
    print(
        "# asm | code | steps | r0-r12 sp lr nzcv | s0-s31 fpscr"
        " | r0-r12 sp lr pc nzcv | s0-s31 fpscr | writes"
    )
    for asm, mode in FP_TEMPLATES:
        code, n = assemble(asm)
        for _ in range(3 * CASES):
            regs, flags = rand_regs(rng, "mem" if mode == "fpmem" else "alu")
            s, fpscr = rand_fp(rng, mode)
            if asm.startswith("vmsr"):
                # Only the modes m33 runs arithmetic in
                regs[0] = (regs[0] & 0xF000009F) | (rng.getrandbits(1) << 25)
            final, nzcv, writes, fp_final = run_case(
                asm, code, regs, flags, mem, (s, fpscr)
            )
            hexes = lambda vs: " ".join(f"{v:x}" for v in vs)
            fields = [
                asm,
                code.hex(),
                str(n),
                hexes(regs + [flags]),
                hexes(s + [fpscr]),
                hexes(final + [nzcv]),
                hexes(fp_final),
                " ".join(f"{a:x}:{s}:{v:x}" for a, s, v in writes),
            ]
            print(" | ".join(fields))


if __name__ == "__main__":
    sys.exit(main())
