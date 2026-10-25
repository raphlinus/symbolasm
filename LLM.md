# On LLM coding with symbolasm

Anecdotally, I've found that Claude Code (Opus 5.5 in particular, so far) can write symbolasm code with great facility. My hypothesis is that symbolasm is a dramatically better target for LLM than raw assembly language. Dan Luu has collected [evidence][How does programming language affect token efficiency and correctness?] that LLM code generation of assembly language significantly underperforms more mainstream languages. I believe symbolasm will close at least some, perhaps most, of that gap.

There are good reasons for this. Raw assembly language is untyped, and the assembler will allow a wide range of nonsensical inputs. By contrast, symbolasm expresses programmer intent much more clearly, facilitates separation of concerns, and, perhaps most importantly, provides quick and accurate feedback.

I strongly suspect that the algebraic surface syntax is a minor contributor; this can be tested through ablation. LLMs are very good at pattern matching and applying the correspondence between `add` and `a + b`. That said, it probably doesn't hurt. Anyone with an understanding of C and Rust will be able to read symbolasm code, and that is no doubt deeply baked into the weights.

One other reason why symbolasm might be well suited for LLMs: compilation speed can be extremely fast, and that is often a bottleneck. I say "can be" because the current compiler has not been carefully optimized for speed or large scale, but the language design supports fast compilation.

## Tool use

One place where the LLM really shines is using tools to guide decisions and validate the generated code. Validation of assembly language is especially important, as there are many things that can go wrong. To this end, symbolasm includes a variety of tools, tuned well for efficient LLM usage.

The analysis tool is produces simple, easy-to-parse textual output, curation of signals that are likely to be useful, and, for the "improvement" part of the tool, providing the output in patch form.

LLMs do well when they have objective targets and a feedback loop. The analysis tool clearly summarizes code size, cycle counts (for individual instructions and loops), as well as reasons for possible performance pitfalls. The LLM can iterate quickly, trying alternatives to see which performs best. Of course, good tools also help human programmers.

Claude also makes very good use of the included Cortex-M33 emulator, which runs as both a command line tool and a library. When I was writing the linked-list allocator (a tricky bit of logic), I was unsure of its correctness, and sure enough Claude was able to find a bug, by writing a test framework around the allocator. It seems likely there are more gains from streamlining and automating emulator-based tests, though it's quite usable now.

## The future

To me, the most exciting future direction is proofs of correctness, particularly of memory safety properties. Currently, symbolasm has a very C-like memory safety story. Testing can only help so much.

As a rough sketch, I see symbolasm growing a Rust-like type system that *expresses* the memory safety properties but does not by itself enforce them. Part of the validation of a function is coming up with a proof that it does indeed respect the safety properties required by the type. The types compose.

Programming at a low level has significant advantages. While efforts to come up with a complete formal semantics for Rust are underway, we have in hand formal semantics for ARM and x86 processors. In addition, proving properties about assembly language programs reduces concerns about compiler correctness.

It feels like this direction is very much worth exploring.

[How does programming language affect token efficiency and correctness?]: https://danluu.com/pl-tokens/
