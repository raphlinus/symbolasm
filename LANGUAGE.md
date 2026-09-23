# Symbolasm language

*This was written early, before implementation. Not everything has been implemented, and there will probably be some changes. This document will very likely split into a general introduction in the README and a reference.*

This is a very low level language, only one step above assembler. A major goal is that every assembler program is expressible. Portability is not a goal, programs are specialized for the target CPU. The initial version is for Cortex-M33, particularly the RP2350 configuration.

Generally, each statement compiles to a single assembly instruction.

## Basic instructions

Here, `e` corresponds to "operand2" in ARM, so can be a register, shifted register, or immediate.

| ARM assembly       | Symbolasm         |
| ------------------ | ----------------- |
| add r0, r1, e      | r0 = r1 + e      |
| adc r0, r1, e      | r0 = r1 + e + #c |
| sub r0, r1, e      | r0 = r1 - e      |
| sbc r0, r1, e      | r0 = r1 - e - !#c |
| rsb r0, r1, e      | r0 = e - r1 |
| rsb r0, r1, #0     | r0 = -r1 |
| rsc r0, r1, e      | r0 = e - r1 - !#c |
| mul r0, r1, r2     | r0 = r1 * r2 |
| mla r0, r1, r2, r3 | r0 = r3 + r1 * r2 |
| mls r0, r1, r2, r3 | r0 = r3 - r1 * r2 |
| udiv r0, r1, r2    | r0 = r1 / r2 |
| sdiv r0, r1, r2    | r0 = (r1 as i32) / r2 |
| mov r0, e          | r0 = e |
| mvn r0, e          | r0 = !e |
| movt r0, c         | r0[16..32] = c |
| asr r0, r1, sh     | r0 = (r1 as i32) >> sh |
| lsl r0, r1, sh     | r0 = r1 << sh |
| ror r0, r1, sh     | r0 = ror(r1, sh) |
| clz r0, r1         | r0 = clz(r1) |
| cmp r0, e          | #(r0 - e) |
| cmn r0, e          | #(r0 + e) |
| tst r0, e          | #(r0 & e) |
| teq r0, e          | #(r0 ^ e) |
| and r0, r1, e      | r0 = r1 & e |
| eor r0, r1, e      | r0 = r1 ^ e |
| orr r0, r1, e      | r0 = r1 \| e |
| orn r0, r1, e      | r0 = r1 \| !e |
| bic r0, r1, e      | r0 = r1 & !e |
| bfc r0, lsb, width | r0[lsb..lsb + width] = 0 |
| bfi r0, r1, lsb, width | r0[lsb..lsb + width] = r1 |
| sbfx r0, r1, lsb, width | r0 = (r1 as i32)[lsb..lsb + width] |
| ubfx r0, r1, lsb, width | r0 = r1[lsb..lsb + width] |
| uxtb r0, r1, ror sh | r0 = r1[i..i + 8] (where i = 0, 8, 16, 24) |
| ldmia r0!, {r1, r2} | (r1, r2) = *r0++ |
| stmia r0!, {r1, r2} | *r0++ = (r1, r2) |
| ldr r0, [r1, r2, lsl #sh] | r0 = *(r1 + (r2 << sh)) |
| ldr r0, [r1, #i]    | r0 = *(r1 + i) |
| ldr r0, [r1], #i    | r0 = *r1; r1 += i |
| ldr r0, [r1], #4    | r0 = *r1++ |
| ldrb r0, [r1]       | r0 = *(r1 as *u8) |
| ldrd r0, r1, [r2]   | (r0, r1) = *r2 |
| ldrd r0, r1, [r2], #8 | (r0, r1) = *r2++ |


A bunch of instructions are expressed as function calls, for example `clz r0, r1` is `r0 = clz(r1)`. These instructions include clz, pkhbt, pkhtb, rbit, rev, rev16, revsh.

Special thing: r0 = &(r1.field) compiles to addition.

Moving a constant to a register will compile to the most efficient sequence. For constants that don't fit the imm8m or imm16 patterns, it will generally be `ldr r0, =addr` with the constant stored in a constant pool, as this is 6 bytes and 1 cycle. (Note: llvm prefers a movt, movw sequence, but this is usually 8 bytes and 2 cycles)

## Variables and placing

A major difference from raw assembler is that variables are named, rather than relying on register numbers. An association between variable and register is formed by a placed assignment: `x @r0 = expr`. Analysis is done to determine the extent of this association. Referring to a variable without a valid association is a compile time error.

To be done: stack placing.

## Types

There are types. These have several functions:

* selecting between signed and unsigned instruction variants (sdiv, sxtb, etc)
* determining the size of memory transfers (ldr, ldrb, ldrh)
* enabling struct field offset syntax: `r0 = r1.field`

I can imagine evolving the type system to support more correctness validation. For example, a u8 type might implicitly carry an assertion that it's in range.

## Pointer arithmetic

Unlike C, pointer arithmetic is by bytes. However, the ++ and -- operators are interpreted as incrementing or decrementing by the size of the transfer.

## 16 bit instructions

There are many cases where a given symbolasm statement could compile to multiple different asm instructions. We pick based on the shortest and most efficient compilation. There will be an analysis to determine whether setting flags is "don't care," and in those cases the flag setting variants are also considered.

One edge case is `r0 = *r1++` which can compile to `stmia r1!, {r0}` or `ldr r0, [r1], #4`. The former is a 16 bit instruction, the latter 32 bit. The semantics are not identical because the former has an alignment requirement. We pick the shorter form. Possibly we'll have a way to express that the longer form should be generated, that it's an unaligned access.
