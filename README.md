# nix2js

This is an experiment to try to transpile nix expressions to JavaScript and
execute them via NodeJS.

This currently evaluates everything strictly,
but encapsulates it using promises, which gives a kind of non-strict eval
with regards to assertions and other exceptions.

The (`target/release`)`nix2js` executable can be built using:
```sh
cargo build --release
```

## WASM

The wasm version of `nix2js` can be built (requires `wasm-pack`, and it's dependencies) using:
```sh
(cd wasm && wasm-pack build --target node)
```
The resulting code is then (if successful) present in `wasm/pkg`.

## REPL

```sh
npm i
# this calls tsc and wasm-pack
npx gulp compile
node
```
inside of the node REPL, type the following to setup some baseline env:
```javascript
// we can import parts of nixpkgs,
// going directly to `impure.nix` avoids confrontation with missing nix-version stuff
// we must use an absolute path for the initial file

let wtf = require('wtfnode');
let nixRtFe = await import('./mock-runtime.mjs');
let a = await nixRtFe.import_(path.resolve(process.env.HOME,'devel/nixpkgs/pkgs/top-level/impure.nix'));
let b = await a({localSystem:{system:'x86_64-linux'}});
```

## TODO

- reintroduce laziness (because just using promises results in too many errors)
- nested attrset keys and non-recursive attrsets are implemented suboptimally
- implement missing nix builtins (esp. those marked with `TODO:`)
  - `derivation` should create an object with a `realise` method
- reduce the call stack size and investigate the use of `Promise` in JS.
- Promise's are incompatible with the current use of `force`...
