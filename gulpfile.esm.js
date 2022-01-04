import { spawn } from 'child_process';
import * as fs from 'fs/promises';
import * as gulp from 'gulp';
import ts from 'gulp-typescript';
import merge from 'merge-stream';

let tsProject = ts.createProject('tsconfig.json');

gulp.task('compile', (cb) => {
    let rustwasm = spawn("wasm-pack", ["build", "--target", "nodejs"], {
        'cwd': 'wasm',
        'stdio': 'inherit',
    });
    rustwasm.on('error', (err) => {
        console.error('failed to start wasm-pack');
        cb(err);
    });
    rustwasm.on('close', (code) => {
        if (code !== 0) {
            console.error('wasm-pack failed! $?=' + code.toString());
            cb(new Error('wasm-pack failed'));
        }
    });
    let tsres = tsProject.src().pipe(tsProject());
    return merge(tsres.dts, tsres.js)
        .pipe(gulp.dest("nix-builtins"));
});

gulp.task('gignsort', async () => {
    let content = await fs.readFile('.gitignore', 'utf8');
    let arr = content.split("\n").filter(a => a != "").sort();
    await fs.writeFile('.gitignore', arr.join("\n") + "\n", 'utf8');
});
