import { spawn } from 'child_process';
import fs from 'fs/promises';
import gulp from 'gulp';
import ts from 'gulp-typescript';
import merge from 'merge-stream';

let tsProject = ts.createProject('tsconfig.json');

let compile_rust = (cb) => {
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
        } else {
            cb();
        }
    });
};
gulp.task('compile-rust', compile_rust);

let compile_ts = () => {
    let tsres = tsProject.src().pipe(tsProject());
    return merge(tsres.dts, tsres.js)
        .pipe(gulp.dest("nix-builtins"));
};
gulp.task('compile-ts', compile_ts);

gulp.task('compile', gulp.parallel(compile_rust, compile_ts))

gulp.task('gignsort', async () => {
    let content = await fs.readFile('.gitignore', 'utf8');
    let arr = content.split("\n").filter(a => a != "").sort();
    await fs.writeFile('.gitignore', arr.join("\n") + "\n", 'utf8');
});
