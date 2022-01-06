import fs from 'node:fs/promises';
import { constants as fsconsts } from 'node:fs';
import path from 'node:path';
import { translate } from 'nix2js-wasm';
import * as nixBlti from 'nix-builtins';

function fmtTdif(tdif) {
    let [secs, nanos] = tdif;
    let millis = nanos / 1000000;
    return secs.toString() + "s " + millis.toFixed(3).toString() + "ms";
}

const rel_path_cache = {};
const import_cache = {};

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
});

async function importTail(real_path) {
    const tstart = process.hrtime();
    let fdat = null;
    try {
        fdat = await fs.readFile(real_path, 'utf8');
    } catch(e) {
        if (e.message.includes('illegal operation on a directory')) {
            real_path = path.resolve(real_path, 'default.nix');
            console.log('   -> retry with: ' + real_path);
            fdat = await fs.readFile(real_path, 'utf8');
        } else {
            console.log(real_path, e);
            throw e;
        }
    }
    console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\tloaded');
    let [trld, srcmap] = translate(fdat, real_path);
    console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\ttranslated');
    let stru;
    try {
        stru = (new Function('nixRt', 'nixBlti', trld));
    } catch (e) {
        console.log(real_path, e);
        throw e;
    }
    try {
        stru = stru(buildRT(real_path), nixBlti);
    } catch (e) {
        console.log(real_path, e);
        throw e;
    }
    console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\tevaluated');
    import_cache[real_path] = stru;
    return stru;
}

export function import_(xpath) {
    if (xpath instanceof Promise)
        return xpath.then(import_);
    if (xpath instanceof Error)
        throw xpath;
    return import_cache[xpath] = importTail(xpath);
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
                    return path.resolve(dirnam, xpath);

                case "Absolute":
                    return path.resolve(xpath);

                case "Home":
                    return path.resolve(process.env.HOME, xpath);

                // weirdly named anchor...
                case "Store":
                    if (!nix_path_parsed) {
                        return Error(opath + ": export not supported: " + anchor + "|" + xpath);
                    }
                    let parts = xpath.split(path.sep);
                    if (nix_path_parsed.lookup[parts[0]] !== undefined) {
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
                        return Error(opath + ": export did not resolve: " + anchor + "|" + xpath)
                    })();

                default:
                    return Error(opath + ": export not supported: " + anchor + "|" + xpath);
            }
        },
        'import': import_,
    };
}
