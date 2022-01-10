import fs from 'node:fs/promises';
import { constants as fsconsts } from 'node:fs';
import path from 'node:path';
import { translate_inline_srcmap } from 'nix2js-wasm';
import * as nixBlti from 'nix-builtins';

function fmtTdif(tdif) {
    let [secs, nanos] = tdif;
    let millis = nanos / 1000000;
    return secs.toString() + "s " + millis.toFixed(3).toString() + "ms";
}

let setImmediatePromise = () => new Promise(resolve => {
    setImmediate(resolve);
});

export const import_cache = new Map();

let nix_path_parsed = (() => {
    let nixpath = process.env.NIX_PATH;
    if (!nixpath) {
        return undefined;
    }
    let parsed = {
        lookup: {},
        rest: [],
    };
    for (const i of nixpath.split(':')) {
        let parts = i.split('=', 2);
        switch(parts.length) {
            case 1:
                parsed.rest.push(i);
                break;
            case 2:
                parsed.lookup[parts[0]] = parts[1];
                break;
        }
    }
    return parsed;
})();

async function importTail(real_path) {
    const tstart = process.hrtime();
    let fdat = null;
    try {
        fdat = await fs.readFile(real_path, 'utf8');
    } catch(e) {
        if (e.message.includes('illegal operation on a directory')) {
            real_path = path.resolve(real_path, 'default.nix');
            fdat = await fs.readFile(real_path, 'utf8');
        } else {
            console.log(real_path, e);
            throw NixEvalError(e.stack);
        }
    }
    try {
        console.log(real_path + '  ' + fmtTdif(process.hrtime(tstart)) + '\tloaded');
        let trld = translate_inline_srcmap(fdat, real_path);
        console.log(real_path + '  ' + fmtTdif(process.hrtime(tstart)) + '\ttranslated');
        let stru;
        stru = (new Function('nixRt', 'nixBlti', trld));
        // call the yield here to allow any hanging events to proceed
        //await setImmediatePromise();
        stru = stru(buildRT(real_path), nixBlti);
        console.log(real_path + '  ' + fmtTdif(process.hrtime(tstart)) + '\tevaluated');
        import_cache.set(real_path, stru);
        console.debug(real_path + '  -res-> ');
        console.debug(stru);
        return stru;
    } catch (e) {
        console.log(real_path, e);
        throw NixEvalError(e.stack);
    }
}

export function import_(xpath) {
    if (xpath instanceof Promise)
        return xpath.then(import_);
    if (xpath instanceof Error)
        throw xpath;
    if (!import_cache.has(xpath)) {
        import_cache.set(xpath, importTail(xpath));
    }
    return import_cache.get(xpath);
}

function buildRT(opath) {
    // get opath directory absolute.
    opath = path.resolve(opath);
    const dirnam = path.dirname(opath);
    return {
        'export': (anchor, xpath) => {
            console.log(opath + ': called RT.export with anchor=' + anchor + ' path=' + xpath);
            //throw Error('loading prohibited');
            if (!xpath) {
                throw Error(opath + ': null path');
            }
            switch (anchor) {
                case "Relative":
                    if (opath === path.resolve(process.env.HOME,'devel/nixpkgs/lib/systems/inspect.nix') && xpath === './parse.nix') {
                        throw Error(opath + ': manual mutual-recursion breaker');
                    }
                    return path.resolve(dirnam, xpath);

                case "Absolute":
                    return path.resolve(xpath);

                case "Home":
                    return path.resolve(process.env.HOME, xpath);

                // weirdly named anchor...
                case "Store":
                    if (!nix_path_parsed) {
                        throw new nixBlti.NixEvalError(opath + ": export not supported: " + anchor + "|" + xpath);
                    }
                    let parts = xpath.split(path.sep);
                    if (nix_path_parsed.lookup.hasOwnProperty(parts[0])) {
                        return path.resolve(nix_path_parsed.lookup[parts[0]], ... parts.slice(1));
                    }
                    return (async () => {
                        for (const i of nix_path_parsed.rest) {
                            let tmp = path.resolve(i, xpath);
                            try {
                                await fs.access(tmp, fsconsts.R_OK);
                                return tmp;
                            } catch(e) { }
                        }
                        throw new nixBlti.NixEvalError(opath + ": export did not resolve: " + anchor + "|" + xpath)
                    })();

                default:
                    throw Error(opath + ": export not supported: " + anchor + "|" + xpath);
            }
        },
        'import': import_,
        'pathExists': async xpath => {
            try {
                await fs.access(await xpath, fsconsts.R_OK);
                return true;
            } catch(e) {
                return false;
            }
        },
    };
}
