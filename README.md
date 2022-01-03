# nix2js

This is an experiment to try to transpile nix expressions to JavaScript and
execute them via NodeJS.

The (`target/release`)`nix2js` executable can be built using:
```sh
cargo build --release
```

## WASM

The wasm version of `nix2js` can be built (requires `wasm-pack`, and it's dependencies) using:
```sh
(cd wasm && wasm-pack build)
```
The resulting code is then (if successful) present in `wasm/pkg`.

## REPL

```sh
npm i
npm -w nix-builtins run prepare
node
```
inside of the node REPL, type the following to setup some baseline env:
```javascript
let fs = require('fs');
let nixBlti = await import('nix-builtins');
let nixRt = (await import('./mock-runtime.mjs')).nixRt;
```
then you can import JS files generated by the `nix2js` rust program:
```javascript
let x = eval(fs.readFileSync('path/to/file.js')+'')(nixRt, nixBlti);
```
