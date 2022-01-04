import * as fs from 'fs';
import * as path from 'path';
import { translate } from 'nix2js-wasm';
import * as nixBlti from 'nix-builtins';

const REL_PFX = "Relative://";
const ABS_PFX = "Absolute://";

let expf = (anchor, xpath) => {
    console.log('called RT.export with anchor=' + anchor + ' path=' + xpath);
    return anchor + '://' + xpath;
};

function fmtTdif(tdif) {
    let [secs, nanos] = tdif;
    let millis = nanos / 1000000;
    return secs.toString() + "s " + millis.toFixed(3).toString() + "ms";
}

function buildRT(opath) {
    // get opath directory absolute.
    opath = path.resolve(opath);
    const dirnam = path.dirname(opath);
    return {
        export: expf,
        import: (xpath) => {
            console.log(opath + ': called RT.import with path=' + xpath);
            let real_path = null;
            if (xpath.startsWith(REL_PFX)) {
                real_path = path.resolve(dirnam, xpath.slice(REL_PFX.length));
            } else if (xpath.startsWith(ABS_PFX)) {
                real_path = path.resolve(xpath.slice(ABS_PFX.length));
            } else {
                throw Error(opath + ": import not supported: " + xpath);
            }
            console.log('  -> resolved to: ' + real_path);
            const tstart = process.hrtime();
            let fdat = null;
            try {
                fdat = fs.readFileSync(real_path, 'utf8');
            } catch(e) {
                if (e.message.includes('illegal operation on a directory')) {
                    real_path = path.resolve(real_path, 'default.nix');
                    console.log('   -> retry with: ' + real_path);
                    fdat = fs.readFileSync(real_path, 'utf8');
                } else {
                    throw e;
                }
            }
            console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\tloaded');
            let [trld, srcmap] = translate(fdat, real_path);
            console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\ttranslated');
            let stru = (new Function('nixRt', 'nixBlti', trld))(buildRT(real_path), nixBlti);
            console.log('  ' + fmtTdif(process.hrtime(tstart)) + '\tevaluated');
            return stru;
        }
    };
}

export const loadInitial = ipath => buildRT(ipath)['import'](ABS_PFX+path.resolve(ipath));
