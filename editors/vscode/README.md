# symbolasm for VS Code

A language server and syntax highlighting for symbolasm (`.sy`) files.

It's still experimental and has rough edges, but is nonetheless extremely
useful for interactive development. Features include:

- Compile errors (though at the moment it bails after the first error)
- Instruction size, cycle count, and generated asm on hover over a line
- Type and register placement on hover over a variable
- Total per-iteration cycle count on hover over a loop
- Inlay hints for cycle count penalties, with reason codes
- Go to definition for many items

## Building and installing

From the symbolasm repository:

```sh
# The server, into ~/.cargo/bin, which should be on PATH
cargo install --path lsp
# The extension's one dependency, vscode-languageclient
(cd editors/vscode && npm ci)
# Package the extension as symbolasm.vsix and install it in VS Code
editors/vscode/package.sh
```

Then reload the VS Code window. After changing the server, run `cargo
install --path lsp` again. The timing analysis runs `arm-none-eabi-as`,
which also needs to be on `PATH`.

## Settings

- `symbolasm.serverPath`: the server binary, `symbolasm-lsp` by default,
  found on `PATH`; or a path, where `~` is the home directory.
- `symbolasm.args`: compiler arguments. The server uses `--svd`, as in
  `["--svd", "/path/to/RP2350.svd"]`, for peripheral registers.
