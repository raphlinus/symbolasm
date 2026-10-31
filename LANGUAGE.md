# Symbolasm language

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
| push {r0, r1, r2}   | push(r0, r1, r2) |
| pop {r0, r1, r2}    | pop(r0, r1, r2)  |


A bunch of instructions are expressed as function calls, for example `clz r0, r1` is `r0 = clz(r1)`. These instructions include clz, pkhbt, pkhtb, rbit, rev, rev16, revsh, ror. The shifts for `pkhbt` and `pkhtb` are written in the second argument, for example `r0 = pkhbt(r1, r2 << 16)` compiles to `pkhbt r0, r1, r2, lsl #16`.

Wrapping an assignment in `#()` is the flag-setting form, for example `#(r0 += r1)` compiles to `adds r0, r0, r1`.

Moving a constant to a register will compile to the most efficient sequence. For constants that don't fit the imm8m or imm16 patterns, it will generally be `ldr r0, =addr` with the constant stored in a constant pool, as this is 6 bytes and 1 cycle. Note: llvm prefers a movt, movw sequence, but this is usually 8 bytes and 2 cycles. The `mvn` instruction is also employed for immediates it covers, most useful for small negative values.

A number of simple no-operand instructions are supported with the same name: nop, wfe, wfi, sev, dmb, dsb, isb. In addition, `cpsie(i)` and `cpsid(i)` are supported, with allowed arguments of `i`, `f`, and `if`.

A move to the same register is elided, an exception to the "one statement compiles to one instruction" general rule. Such statements are useful for more flexible variable placement.

### Branching

| ARM assembly       | Symbolasm         |
| ------------------ | ----------------- |
| b label            | goto label        |
| beq label          | if #eq goto label |
| cbz r0, label      | if r0 == 0 goto label |
| cbnz r0, label     | if r0 != 0 goto label |
| bx lr              | bx lr             |
| blx r0             | blx r0            |

The `bl label` syntax is supported for raw calls, but most calls will be done using `call` syntax, described below.

### Block structure

One of the things that makes assembly language programs hard to read and reason about is that control flow is expressed as a linear sequence of instructions, with labels and branches. In the '60s and '70s, the debate around "structured programming" raged, and these days control flow is almost always expressed using `if/else` and loops including `while`, with `goto` in a secondary role.

Symbolasm provides block structure as a way of expressing programmer intent, while also preserving the explicit mapping to processor instructions. The block structure syntax brings specific labels into local scope, but does not itself insert any branching instructions; these are still programmer responsibility.

The `if { a } else { b }` syntax brings the label "end" into local scope, and "else" is in scope for the first block. Thus, writing "if r0 == 0 { r1 = 0 } else { r1 = 1 }" can be written:

```
    if {
        if r0 != 0 goto else
        r1 = 0
        goto end
    } else {
        r1 = 1
    }
```

The statement before the `else` block must be a branch.

Similarly, `loop { a }` syntax brings `loop` and `break` labels into scope, representing the top and bottom of the loop, respectively.

Lastly, `while { a } loop { b }` brings `while`, `loop`, and `break` into scope. For example, printing the numbers 0..n can be written:

```
    while {
        if n == 0 goto break
        i @r4 = 0
    } loop {
        i_print @r0 = i
        call print(i_print)
        i += 1
        #(i - n)
        if #ne goto loop
    }
```

### Conditional execution

The `it` instruction is supported with `if` statement syntax. For example:

```
    if #eq {
        r0 = 0
    } else {
        r0 = 1
    }
```

compiles to:

```
    ite eq
    moveq r0, #0
    movne r0, #1
```

Currently not supported is interleaving of `then` and `else`, for example `itet`.

## Function declarations

A basic function declaration is `fn name(args) -> ret { body }`. The return value is optional, and can be a tuple. Each argument is either `variable` or `variable: type`; in the former case, it defaults to `u32`.

Labels are local to the function body. Branches to labels not locally declared are considered to be global.

A function can be annotated with an ABI, using `fn[abi = loose] name(args) -> ret` syntax. Three initially supported values are aapcs, loose, and wild. Default is aapcs. We may expand this set, or allow for more fine grained customization, in the future.

The wild ABI potentially clobbers all registers, can leave stack unbalanced, and doesn't require stack analysis to converge.

The loose ABI is similar to aapcs but r4-r7 are caller-saved, up to 8 arguments can be passed in registers, and up to 8 registers are available for tuple return values. In addition, stack is only 4 byte aligned.

Stack alignment is checked; calling an aapcs function requires the caller to be aapcs as well, and that the stack offset is 0 mod 8.

Functions are called with `ret @r0 = call target(arg0, arg1)` syntax. It is an error for the args and return values to be placed in registers other than ABI sequence, or for length to mismatch. Explicit placement of return value is optional when the variable is already placed in the correct register. The `bl` syntax is still allowed on defined symbols, and does no checking.

Function types of the form `fn(args) -> ret` are supported, as a form of pointer (very useful for vtables). Calling a function pointer stored in a register compiles to `blx` and invokes the same checks as a static function of the same type. ABI defaults to aapcs but can be modified with an annotation, as in `fn[abi = loose]()`

The return value in the function signature is implicitly placed in their register at each assignment within the function body (killing any other placement in that register). Only assignment with `=` counts for this purpose, not `+=` and so on.

Arguments passed on stack are given as stack-placed variables, and must be at the correct stack offset. Example (this is a code fragment):

```
    arg0 @r0 = 0
    arg1 @r1 = 1
    arg2 @r2 = 2
    arg3 @r3 = 3
    arg4 @r12 = 4
    push(arg4)
    call function(arg0, arg1, arg2, arg3, sp.arg4)
    sp += 4
```

There is no ordering constraint that a function needs to be declared before its use, or in the same file. Using a function in another file requires that file to be listed in a "use" item, however.

A `symbol` definition matching a function is allowed (typically in different files), allowing for more flexible linking. The ABI, argument types, and return value types must match.

If and when we do constant placement, it will be allowed in arg position, as in `call print_char(42 @r0)`. I can see this being very useful. More complex expressions will not be allowed, unless in the future we have a relaxed mode.

Possible future extension: `#[must_use]` annotation, which would trigger a lint for unused return values.

Tail calls are supported with `#[tail] call` syntax. This checks the same as a normal call, including that return value signature matches the caller. It compiles to the same instruction as `goto` but signals intent and is more thoroughly checked.

## Variables and placing

A major difference from raw assembler is that variables are named, rather than relying on register numbers. An association between variable and register is formed by a placed assignment: `x @r0 = expr`. Analysis is done to determine the extent of this association. Referring to a variable without a valid association is a compile time error.

A variable always has the same type within a function, but can be placed in different registers at different points in the function.

## Stack placement

Stack placement is similar to register placement, but with some important differences.

### Stack analysis

Stack analysis is a flow analysis that assigns each instruction a stack offset relative to the start of the function. Failing to converge is an error unless ABI is "wild". Increment/decrement by register is forbidden outside "wild".

When a variable is stack-placed, it is accessible using `sp.x` syntax – load, store, and address-of, all of which have ARM instruction support.

ABI spill arguments are stack-placed on function entry.

Pushing a variable stack-places it, while keeping the register placement. Register placements and stack placements are separate namespaces; `x` refers to a register, while `sp.x` syntax is needed to refer to the stack placement.

Variables can be stack-placed, uninitialized, using the following syntax:

```
    sp -= 12 (x, p: Point)
```

The variables are placed lowest-first, matching push. Each variable is aligned according to its type, and the total is rounded up to a multiple of 4. It is an error for the (rounded) size of the tuple to mismatch the decrement.

Popping or incrementing sp un-places the variables in the affected range. If it splits a placement of size larger than 4, the entire placement is killed (this might be a lint).

Other sp-relative writeback forms are analyzed the same way as push/pop.

Pushing and popping a numbered register which contained a variable places the variable back in that register. The same core analysis applies to verifying callee-saved registers; callee-saved registers can be analyzed as pseudo-variables which must retain their valid placement on function exit.

Assignment to the stack using mechanisms other than stack placement (including numeric offsets) does not affect placement. While it would be possible to analyze some of these patterns, it is in general undecidable. Accessing the stack in this way signals programmer intent to do something nonstandard.

## Types

There are types. These have several functions:

* selecting between signed and unsigned instruction variants (sdiv, sxtb, etc)
* determining the size of memory transfers (ldr, ldrb, ldrh)
* enabling struct field offset syntax: `r0 = r1.field`

The built-in scalar types are `u32` (default), `u16`, `u8`, `i32`, `i16`, `i8`, `f32`. Pointers are `*T`. As in Rust, `as` syntax casts from one type to another.

Array types are supported, with `[T; n]` syntax. That said, there is no dedicated syntax for array indexing; this is expressed using pointer dereferencing. For an array type, `count()` represents the number of elements. For all types, `size(name)` is its size in bytes (name can refer to a static, a scalar type, or a struct).

I can imagine evolving the type system to support more correctness validation. For example, a u8 type might implicitly carry an assertion that it's in the 0..256 range.

## Pointer arithmetic

Unlike C, pointer arithmetic is by bytes. However, the ++ and -- operators are interpreted as incrementing or decrementing by the size of the transfer.

## Structs and data

Types include C-style structs. The syntax is as follows:

```
struct Point {
    x: u32
    y: u32
}
```

It's possible to place structs and other types in memory, using `static`:

```
static MyPoint = Point {
    x: 1,
    y: 2,
}

static mut LastPoint: Point
```

A `#[link_section = ".ram"]` annotation places the item in that section; `static mut` is placed in `.data` by default if it has an initializer, otherwise `.bss` (zero initialized), while `static` defaults to `.text`, and requires an initializer.

Array initializers work with the expected syntax. If multiline, trailing commas are required. If on one line, the trailing comma is forbidden.

Data initializers can take an `#[align = n]` attribute. Attributes stack.

Struct field access is supported, including nested structs. These compile to the appropriate `ldr` or `str` instruction variant. Taking the address of a struct field is also supported (for example `&s.field`) and compiles to addition.

## Floating point

Floating point is now supported, including basic arithmetic. The float type is `f32` (currently no support for `f64`). Float registers are `s0` through `s31`; these are reserved and can't be used to name variables.

Literals must be floating point. Some literals can be expressed as immediates, others (including 0.0) are loaded from the constant pool, analogous to integers.

The following intrinsics have the same name and semantics as in Rust: `abs sqrt floor ceil round round_ties_even trunc min max`. The `to_bits` and `from_bits` intrinsics do bit casting. In addition, `i32_to_f32` and `f32_to_i32` do conversion, as do their `u32` counterparts. Rounding can be combined with conversion to integer in a single instruction, as in `f32_to_i32(floor(x))`. Using `as` to cast float types is not supported, but type punning through a pointer cast will work.

The `fma` intrinsic is supported, with both `fma(a, b, x)` and `fma(-a, b, x)` supported. This is a fused multiply-add. Non-fused multiply-adds are supported using `x += a * b`, `x -= a * b`, `x = a * b - x`, and `x = -x - a * b`. Note: the result must be stored in the same register as the addend; the underlying instructions are 3-register.

The `push` and `pop` instructions are supported. Integer and float registers cannot be mixed.

Loads and stores work, with integer offsets (multiples of 4 up to ±1020), struct fields, stack, and post-increment. Tuples must be consecutive registers. There are no instructions for register offset or post-decrement.

Compares are slightly awkward on Cortex-M33. The main comparison instruction is `fpscr = cmp(a, b)`, which is then best transferred to processor flags using `#(fpscr)`. The second argument can be a register or `0.0`. The `cmpe()` variant (raising an Invalid Operation by setting the sticky IOC bit in fpscr) is also provided. In addition `r = fpscr` and `fpscr = r` are supported. The semantics of comparison are fiddly: an unordered comparison makes `#lt` true and `#mi` false. The `#gt` and `#ge` tests are false on unordered comparisons. Unordered comparisons set the `V` flag and so can be tested with `#vs`.

The ABI is hard-float and follows ARM conventions: `s0`-`s15` are caller-saved and `s16`-`s31` are callee-saved, in both `aapcs` and `loose` ABIs.

## Const

The `const` item defines a constant, which can then be used in expressions. For example, `const LED_PIN = 29` enables `pin @r0 = LED_PIN` later.

The compiler applies constant folding and some canonicalization of expressions. For example, multiplication by a constant power of two is rewritten to shift; `x * 4` becomes `x << 2`.

## 16 bit instructions

There are many cases where a given symbolasm statement could compile to multiple different asm instructions. We pick based on the shortest and most efficient compilation. There is an analysis to determine whether setting flags is "don't care," and in those cases the flag setting variants are also considered.

One edge case is `r0 = *r1++` which can compile to `ldmia r1!, {r0}` or `ldr r0, [r1], #4`. The former is a 16 bit instruction, the latter 32 bit. The semantics are not identical because the former has an alignment requirement. We pick the shorter form. Possibly we'll have a way to express that the longer form should be generated, that it's an unaligned access.

## Linker symbols

The top level item `symbol name: type` populates a global symbol, which is expected to be satisfied by the linker, typically one defined in the linker script such as `_data_start`.

This mechanism has a similar scope as `extern` in C and Rust, but with a difference: `extern` refers to the object, while `symbol` refers to its address. For example, `symbol _data_start: *u32` is a pointer, and `symbol _data_size_words: u32` is a number.

A symbol can be used in the following contexts:

| ARM assembly            | Symbolasm                  |
| ----------------------- | -------------------------- |
| ldr r0, =name           | r0 = name                  |
| mov r0, #:lower16:name  | r0 = lower16(name)         |
| movt r0, #:upper16:name | r0[16..32] = upper16(name) |

## Importing SVD

The compiler has a command line option to import an [SVD] (System View Description) file. This populates addresses and types.

There are still some gaps. Named bitfield ranges and enums are under consideration but not yet implemented.

## Explicit dependencies

Files can refer to items defined by other files, but need an explicit `use "otherfile.sy"` before they're used (usually at the top of the file). Among other things, this enables checking of a single file without needing a manifest.

## Not yet implemented, but planned

The `adc` and `sbc` instructions don't work (syntax in above table is provisional).

[SVD]: https://arm-software.github.io/CMSIS_5/SVD/html/index.html
