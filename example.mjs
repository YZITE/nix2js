import * as nixRtFe from './mock-runtime.mjs';
import path from 'node:path';

let a = await nixRtFe.import_(path.resolve(process.env.HOME,'devel/nixpkgs/pkgs/top-level/impure.nix'));
a({localSystem:{system:'x86_64-linux'}}).then(console.log);
