# symbolasm

A very low level language for programming Cortex-M33 microcontrollers.

## Background

Assembly language was mainstream in the early days of computing, but has fallen almost completely out of style. For most low level programming tasks, an optimizing C compiler fits the bill. The biggest exception is cryptographic primitives, which require both absolute maximum performance and also control over things like timing attacks and whether memory actually gets zeroed before freeing, neither of which can be expressed in standard C.

Even so, I personally find assembly language appealing, especially on microcontrollers. These chips are simple enough that it's *possible* to reason about performance, and I enjoy the challenge of pushing them to see what performance is possible. My [vic-2350] project, a race-the-beam renderer for the RP2350, would not work without at least hand-optimized rendering kernels.

Assembly language hasn't advanced much since around the mid-1950s. The [Symbolic Optimal Assembly Program][SOAP] is not wildly different from assemblers today, with three letter mnemonics such as `MPY` (multiply) and `DIV` (divide), and even `BMI` (branch minus) still has the same name on a modern ARM chip (the 64-bit version makes the dramatic leap to `B.MI`). This assembler also used names for labels and global variables. Globals are also less prominent in modern programming, so using numeric offsets to reference stack-placed variables and field offsets in structs is actually something of an ergonomic regression.

And assembly language has a well-earned reputation for being tedious and difficult to get right. What if we brought modern compiler techniques to bear, creating a language that much more clearly expresses programmer intent, while preserving the ability to reason about low-level performance? The result is symbolasm, or "symbolic assembly."

The design is guided by some overriding principles:

* It is possible to represent the most efficient program.
* The surface syntax should clearly express the program logic and data structures.
* The compiler gives rich feedback.
* There is a direct correspondence between the program and generated assembly.

The most important non-goal is portability, which is in tension with the previous ones, but more about that later.

See [LANGUAGE.md](LANGUAGE.md) for a brief description of the language.

## Example

Here's an extended real example, taken from a linked-list allocator in the [vic-2350] repo.

```rust
struct Hole {
    next: *Hole
    size: u32
}

static mut alloc_head: Hole

// Free a block allocated with this size.
fn[abi = loose] free(ptr: *Hole, size: u32) {
    head @r2 = &alloc_head
    size += 7
    cur @r3 = head.next
    #(size &= !7)
    if #eq {
        size = 8
    }
    // Stop when cur is past ptr or null: cur - 1 >= ptr, unsigned,
    // covers both.
    prev @r2 = head
    loop {
        t @r4 = cur - 1
        #(t - ptr)
        if #hs goto break
        prev = cur
        cur = prev.next
        goto loop
    }
    // prev < ptr < cur (cur may be 0); first try to absorb cur
    ptr_end @r4 = ptr + size
    #(ptr_end - cur)
    prev_size @r5 = prev.size
    if {
        if #ne goto end
        cur_size @r4 = cur.size
        cur = cur.next
        size += cur_size
    }
    // then try to extend prev over the freed block
    prev_end @r4 = prev + prev_size
    if {
        #(prev_end - ptr)
        if #ne goto else
        prev_size += size
        prev.size = prev_size
        prev.next = cur
        bx lr
    } else {
        ptr.size = size
        ptr.next = cur
        prev.next = ptr
        bx lr
    }
}
```

A few things special to symbolasm. The `@r4` syntax places the variable in that register. Placements can be dynamic, and a flow analysis ensures they are consistent; trying to place two variables in the same register at the same time is a compile time error. The `#()` syntax sets flags, and can either be a comparison or an asssignment operation. And the block structure only establishes local labels; it is still up to the programmer to add the conditional branches.

## Capturing programmer intent

A major theme in symbolasm is capturing and expressing programmer intent, much of which is obscured or even erased in assembly language.

Most notably, variables are named, as opposed to being anonymous, distinguished only by register number or position on the stack. Register allocation is still explicit, but using syntax (`x @r6 = ...`) that separates the concerns; the program still makes sense even if all the placements were erased.

Similarly, while assembly language is known for being essentially untyped, variables, arguments, and data structures in symbolasm have C-like types. Struct fields are accessed by name, not numeric offset.

## Why RP2350?

The *idea* of symbolasm would work on pretty much any modern CPU, but conditions on the RP2350 are especially favorable.

For one, portability becomes much less important, as the RP2350 has a unique set of peripherals (HSTX for DVI output, PIO). For programs that make use of these peripherals (such as my beam racer), it doesn't make sense to compile it for another chip.

For two, optimizing for code size is especially important, for two main reasons. There's only 520kB of SRAM, and timing critical code and data needs to reside there. Wasting precious RAM is not an option. There's lots more space in flash and PSRAM (16MB and 8MB respectively), but then there's another problem: on cache miss, running code out of flash (XIP or execute-in-place) is about 20 times slower than from SRAM. Performance is almost strictly determined by code size, so the usual compiler optimization tricks of inlining and loop unrolling can actually make things worse. By contrast, on larger systems, it is unusual for instruction cache misses to affect performance measurably at all.

## Tools

A compiler is only a small part of a language ecosystem. I'm starting to build a set of tools to help with development.

At the core is an analysis tool. It works in batch mode, outputting a plain ASCII file. Here's an excerpt from the above example:

```
  110:                                                 loop {
  111: 1            1 subs r4, r3, #1                      t @r4 = cur - 1
  112: 4            1 cmp r4, r0                           #(t - ptr)
  113: d    K     0/2 bhs .Lfree_0_break                   if #hs goto break
  114: 4            1 mov r2, r3                           prev = cur
  115: 6            1 ldr r3, [r2, #0]                     cur = prev.next
  116: e            1 b .Lfree_0_loop                      goto loop
  117:                                                 }
   loop 111-116: 5 cycles per iteration
```

The first column is the first hex digit of the code; a 32 bit instruction would be two digits, but here all are 16 bit. The 'K' is the reason why the branch takes 2 cycles when taken; the condition is not known at the start of the previous instruction.

This analysis shows one feature of the language: with few exceptions, each source statement compiles to exactly one assembly language instruction. Fortunately, ARM instructions are quite expressive.

The tool also has an LSP, visualizing the same information on hover and in inlay hints.

Cycle counts are based on a carefully measured model, taking into account interactions between instructions such as result-use delays.

The repo also contains a Cortex-M33 emulator (in the m33 subdirectory), which generates memory traces and instruction counts (the cycle counting model is not yet wired up). It can be used as a command line tool, and also as a library.

On a more experimental basis, the tool tries to do optimized register allocation, either improving an existing allocation or filling in missing placements.

## Revisiting the portability non-goal

[TODO]

* Possibly target other chips with same concept
  + RISC-V is broadly similar, but conditional branching is different

* An optimizing compiler from one symbolasm to another
  + Doesn't guarantee 100% optimal performance but likely very good

* Possibly a "less strict" mode that, for example, expands `if a > b goto label` to two instructions on ARM (`#(a - b)`, `if #hi goto label`)

Mention [The scourge of x86 emulation]. This is a problem because on x86 assembly language, the distinction between relaxed and acquire/release atomics is erased. If the intent were preserved (which will be done in symbolasm, though atomics haven't landed yet), translation would be much more efficient.

## LLMs

I originally designed symbolasm to be enjoyed by human programmers (including myself). However, part of the motivation for continuing the work is finding that it works surprisingly well with LLM-assisted coding. Because not everybody will appreciate this direction, I've made a separate [LLM.md](LLM.md) page.

## Related work

I found surprisingly little related work with the same goals. There were a number of variations of "[high level assembly][HLA]," but these generally have unoptimized code generation, so are handily beat by a good C compiler.

Probably the closest is [qhasm] by Dan Bernstein, used primarily for crypto primitives and not larger systems. This language uses named variables and algebraic syntax. There are a number of more modern takes, including [Vale] and [Jasmin], with a focus on formal correctness.

A more C-like algebraic syntax, as opposed to mnemonics, is used in the assembly language for [Blackfin]. However, registers are still numbered.

Also very much worth mentioning is [wiz], which has a similar philosophy, but makes many detailed decisions differently because it targets 8 bit computers, a much more restricted environment than ARM.

## Possible future directions

[TODO: write in more detail]

* Target more CPUs

* Make type system even more Rust-like, with borrowing etc

* Provide higher level language (to be run on host) for tests

* Facilitate formal proofs of correctness
  + Lower level and simpler could be easier to formalize - formal semantics exist for arm & x86, but not yet Rust. Stuff like provenance is hard

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

[vic-2350]: https://github.com/raphlinus/vic-2350
[SOAP]: https://en.wikipedia.org/wiki/Symbolic_Optimal_Assembly_Program
[HLA]: https://en.wikipedia.org/wiki/High_Level_Assembly
[qhasm]: https://cr.yp.to/qhasm.html
[Jasmin]: https://github.com/jasmin-lang/jasmin
[Vale]: https://github.com/project-everest/vale
[Blackfin]: https://en.wikipedia.org/wiki/Blackfin
[wiz]: https://github.com/wiz-lang/wiz
[The scourge of x86 emulation]: https://fex-emu.com/Scourge-of-emulation/
