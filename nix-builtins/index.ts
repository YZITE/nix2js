// SPDX-License-Identifier: LGPL-2.1-or-later

import * as _ from 'lodash-es';
import assert from 'webassert';

export const API_VERSION = 0;

export class NixAbortError extends Error { }
export class NixEvalError extends Error { }

const LazyHandler = {
    get: (target, key) => (key in target) ? target[key] : target.evaluate()[key],
    has: (target, key) => (key in target) || (key in target.evaluate()),
};

export class Lazy {
    private i: any;
    private iL: boolean;
    constructor(inner) {
        // TODO: maybe integrate Promise objects
        this.i = inner;
        let toi = typeof inner;
        this.iL = toi === "function";
        if (toi === 'object') {
            // automatic unfolding
            return inner;
        } else {
            // automatic attrset unfolding
            return new Proxy(this, LazyHandler);
        }
    }
    evaluate() {
        while (this.iL) {
            this.iL = false;
            let buf = this.i;
            // poison inner Apply.
            this.i = ()=>{throw new NixEvalError('self-referential lazy evaluation is forbidden')};
            this.i._poison = true;
            try {
                let res = buf.apply(buf, arguments);
                // automatic unfolding
                if (res instanceof Lazy) {
                    this.iL = res.iL;
                    this.i = res.i;
                } else {
                    this.i = res;
                    break;
                }
            } finally {
                if (this.i instanceof Function && this.i._poison === true) {
                    // restore the function in case of failure
                    this.i = buf;
                }
            }
        }
        return this.i;
    }
}

// TODO: add class for StringWithContext, although that might be unnecessary,
// because we don't serialize derivations before submit...

export const force = value => (value instanceof Lazy) ? value.evaluate() : value;
const onlyUnique = (value, index, self) => self.indexOf(value) == index;
const fmt_fname = fname => (fname.length != 1) ? fname : ('operator ' + fname);
const natyforce = (objty, natty, ax) => function(val) {
    if(val instanceof Lazy) {
        val = val.evaluate();
    }
    if(val instanceof objty) {
        val = val.valueOf();
    }
    if(typeof val !== natty) {
        throw TypeError('value is ' + typeof val + ' while '+ax+' '+natty+' was expected');
    }
    return val;
};
const otyforce = (objty, ax) => function(val) {
    if(val instanceof Lazy) {
        val = val.evaluate();
    }
    if(typeof val !== 'object') {
        throw TypeError('value is ' + typeof val + ' while an object was expected');
    }
    if(!(val instanceof objty)) {
        throw TypeError('value is ' + val.constructor.name + ' while '+ax+' '+ objty.constructor.name +' was expected');
    }
    return val;
};
const discardStringContext = s => s;
const tyforce_string = natyforce(String, 'string', 'a');
const tyforce_number = val => (typeof val === 'bigint')?val:(natyforce(Number, 'number', 'a')(val));
const tyforce_list = otyforce(Array, 'an');

const isnaty = (objty, natty) => val => ((val instanceof objty) || (typeof val === natty));
const isBool = isnaty(Boolean, 'boolean');
const isNumber = isnaty(Number, 'number');
const isString = isnaty(String, 'string');

// the assignment ensures that future assignments won't currupt the prototype
export const fixObjectProto = (...objs) => Object.assign(Object.create(null), ...objs);

export class ScopeError extends Error { }

// used to get all keys present in a scope, including inherited ones
export const allKeys = Symbol('__all__');

// used to get the current scope, but detached from it's parent scope and
// without the proxy wrapper.
export const extractScope = Symbol('__dict__');

export function mkScope(orig: undefined | null | object): object {
    if (orig === undefined) {
        // "Object prototype may only be an Object or null"
        orig = null;
    }
    // we need to handle mkScope()
    let orig_keys = orig ? (() => Object.keys(orig)) : (() => []);
    // Object.create prevents prototype pollution
    let current = Object.create(orig);
    // self-referential properties
    Object.defineProperty(current, allKeys, {
        get:()=>Object.keys(current).concat(orig_keys()).filter(onlyUnique),
    });
    Object.defineProperty(current, extractScope, {
        get:()=>fixObjectProto(current),
    });
    return new Proxy(current, {
        set: function(target, key, value) {
            if (key == '__proto__')
                throw new ScopeError("Tried modifying prototype");
            let ret = !Object.prototype.hasOwnProperty.call(target, key);
            if (ret) {
                Object.defineProperty(target, key, {
                    value,
                    configurable: false,
                    enumerable: true,
                    writable: false,
                });
            }
            return ret;
        }
    });
}

// mark an object as read-only
const readOnlyHandler = {
    set: function(target, key, value) {
        throw new ScopeError("Tried overwriting key '" + key + "' in read-only scope");
    },
    deleteProperty: function(target, key) {
        if (key in target) {
            throw new ScopeError("Tried removing key '" + key + "' from read-only scope");
        } else {
            return false;
        }
    },
    defineProperty: (target, key, desc) => {
        throw new ScopeError("Tried modifying property '" + key + "' on read-only scope");
    },
    getOwnPropertyDescriptor: function(target, key) {
        let tmp = Object.getOwnPropertyDescriptor(target, key);
        if (tmp) {
            tmp.writable = false;
        }
        return tmp;
    }
};

export function mkScopeWith(...objs: object[]): object {
    let handler = Object.create(readOnlyHandler);
    handler.get = (target, key) => {
        if (key in target) {
            return target[key];
        } else {
            let tmp = objs.find(obj => key in obj);
            return (tmp !== undefined) ? tmp[key] : undefined;
        }
    };
    handler.has = (target, key) => (key in target) || objs.some(obj => key in obj);
    return new Proxy(Object.create(null, {
        [allKeys]: {
            get: () => objs
                .map(obj=>obj[allKeys])
                .flat()
                .filter(i=>i!==undefined)
                .filter(onlyUnique)
        }
    }), handler);
}

const splitVersion = s => s
    .split(/[^A-Za-z0-9]/)
    .map(x => x.split(/([A-Za-z]+|[0-9]+)/).filter((elem,idx) => idx%2))
    .flat();

export function orDefault(lazy_selop, lazy_dfl) {
    let ret;
    try {
        ret = lazy_selop.evaluate();
    } catch (e) {
        // this is flaky...
        if (e instanceof TypeError && e.message.startsWith('Cannot read properties of undefined ')) {
            console.debug("nix-blti.orDefault: encountered+catched TypeError:", e);
            return lazy_dfl.evaluate();
        } else {
            throw e;
        }
    }
    if (ret === undefined) {
        ret = lazy_dfl.evaluate();
    }
    return ret;
}

const binop_helper = (fname, f) => function(a, c) {
    let b = force(a);
    let d = force(c);
    let tb = typeof b;
    let td = typeof d;
    if (tb === td) {
        return f(b, d);
    } else {
        throw TypeError(fmt_fname(fname) + ": given types mismatch (" + tb + " != " + td + ")");
    }
};

function req_type(fname, x, xptype) {
    if (typeof x !== xptype) {
        throw TypeError(fmt_fname(fname) + ": invalid input type (" + typeof x + "), expected (" + xptype + ")");
    }
}

const isAttrs = e => typeof e === 'object' && !(
       (e instanceof Boolean)
    || (e instanceof Number)
    || (e instanceof String)
    || (e instanceof Lazy)
);

const deepSeq_helper = e => {
    e = force(e);
    if (isAttrs(e)) {
        for (let i of e) deepSeq_helper(i);
    }
};

/* @preserve
anti-prototype pollution filter taken from npm package 'no-pollution'
source: https://github.com/DaniAkash/no-pollution/blob/3bfe3f419d49acd1ab157c7b9655161c4942fedd/index.js
Copyright (c) 2019-present DaniAkash
SPDX-License-Identifier: MIT

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
const anti_pollution = e => fixObjectProto(JSON.parse(e.replace(new RegExp("(_|\\\\_|[\\\\u05fFx]+){2}proto(_|\\\\_|[\\\\u05fFx]+){2}(?=[\"']+[\\s:]+[{\"'])", 'g'), '__pollutants__')));

const nixToStringHandler = {
    'object': function(x) {
        // TODO: handle paths
        // "A list, in which case the string representations of its elements are joined with spaces."
        if (x instanceof Array) return x.map(nixToString).join(' ');
        if ('toString' in x) return x.toString();
        if ('__toString' in x) return x['__toString'](x);
        if ('outPath' in x) return x['outPath'];
        throw new NixEvalError('nixToString: unserializable object type ' + x.constructor.name);
    },
    'string': x => x,
    'bigint': x => x.toString(),
    'number': x => x.toString(),
    'boolean': x => x ? "1" : "",
};

function nixToString(x): string {
    x = force(x);
    if (x === null || x === undefined) return "";
    if (typeof x === 'object' && 'valueOf' in x)
        x = x.valueOf();
    if (nixToStringHandler.hasOwnProperty(typeof x)) {
        return nixToStringHandler[typeof x](x);
    }
    throw new NixEvalError('nixToString: unserializable type ' + typeof x);
}

const nixTypeOf = {
    'bigint': 'int',
    'boolean': 'bool',
    'function': 'lambda',
    'number': 'float',
    'object': 'set'
};

// operators
export const nixOp = {
    u_Invert: a => !force(a),
    u_Negate: a => -force(a),
    _deepMerge: function(attrs_: object, value: any, ...path: string[]): void {
        let attrs = attrs_;
        while(1) {
            let pfi = path.shift();
            if (pfi === undefined) {
                throw new NixEvalError("deepMerge: encountered empty path");
            }
            if (typeof attrs !== 'object') {
                throw new NixEvalError("deepMerge: tried to merge attrset into non-object (" + typeof attrs + ")");
            }
            if (path.length) {
                if (!attrs.hasOwnProperty(pfi)) {
                    // this should prevent prototype pollution
                    attrs[pfi] = Object.create(null);
                }
                attrs = attrs[pfi];
            } else {
                attrs[pfi] = value;
                break;
            }
        }
    },
    _lambdaA2chk: function(attrs: object, key: string): any {
        if (attrs[key] === undefined) {
            // TODO: adjust error message to what Nix currently issues.
            throw new NixEvalError("Attrset element " + key + "missing at lambda call");
        }
        return attrs[key];
    },
    Concat: binop_helper("operator ++", function(a, b) {
        if (typeof a !== 'object') {
            throw TypeError("operator ++: invalid input type (" + typeof a + ")");
        }
        return a.concat(b);
    }),
    // IsSet is implemented via .hasOwnProperty
    Update: binop_helper("operator //", function(a, b) {
        if (typeof a !== 'object') {
            throw TypeError("operator //: invalid input type (" + typeof a + ")");
        }
        return fixObjectProto({}, a, b);
    }),
    Add: binop_helper("+", function(a, b) {
        return a + b;
    }),
    Sub: binop_helper("-", function(a, b) {
        req_type("-", a, "number");
        return a - b;
    }),
    Mul: binop_helper("*", function(a, b) {
        req_type("*", a, "number");
        return a * b;
    }),
    Div: binop_helper("/", function(a, b) {
        req_type("/", a, "number");
        if (!b) {
            throw RangeError('Division by zero');
        }
        return a / b;
    }),
    And: binop_helper("&&", function(a, b) {
        req_type("&&", a, "boolean");
        return a && b;
    }),
    Implication: binop_helper("->", function(a, b) {
        req_type("->", a, "boolean");
        return (!a) || b;
    }),
    Or: binop_helper("||", function(a, b) {
        req_type("||", a, "boolean");
        return a || b;
    }),
    Equal: (a, b) => _.isEqual(force(a), force(b)),
    NotEqual: (a, b) => !_.isEqual(force(a), force(b)),
    Less: binop_helper("<", function(a, b) {
        req_type("<", a, "number");
        return a < b;
    }),
    LessOrEq: binop_helper("<=", function(a, b) {
        req_type("<=", a, "number");
        return a <= b;
    }),
    More: binop_helper(">", function(a, b) {
        req_type(">", a, "number");
        return a > b;
    }),
    MoreOrEq: binop_helper(">=", function(a, b) {
        req_type(">=", a, "number");
        return a >= b;
    })
};

export function initRtDep(nixRt) {
    return {
        abort: s => {throw new NixAbortError(tyforce_string(s));},
        add: a => function(c) {
            let b = force(a);
            let d = force(c);
            let tb = typeof b;
            let td = typeof d;
            if (tb === td) {
                req_type("builtins.add", b, "number");
                return b + d;
            } else {
                throw TypeError("builtins.add: given types mismatch (" + tb + " != " + td + ")");
            }
        },
        all: pred => list => Array.prototype.every.call(force(list), x=>force(force(pred)(x))),
        any: pred => list => Array.prototype.some.call (force(list), x=>force(force(pred)(x))),
        assert: function(cond) {
            const cond2 = force(cond);
            if (typeof cond2 !== 'boolean') {
                throw TypeError("Assertion condition has wrong type (" + typeof cond2 + ")");
            }
            assert (cond2);
        },
        attrNames:  aset => Object.keys(force(aset)).sort(),
        attrValues: aset => Object.entries(force(aset)).sort().map(a => a[1]),
        baseNameOf: s => _.last(tyforce_string(s).split('/')),
        bitAnd: v1 => v2 => tyforce_number(v1) & tyforce_number(v2),
        bitOr:  v1 => v2 => tyforce_number(v1) | tyforce_number(v2),
        catAttrs: s => list => {
            const s2 = tyforce_string(s);
            return tyforce_list(list)
                .filter(aset => Object.prototype.hasOwnProperty.call(aset, s2))
                .map(aset => aset[s2]);
        },
        ceil: n => Math.ceil(tyforce_number(n)),
        compareVersions: s1 => s2 => {
            let s1p = splitVersion(tyforce_string(s1));
            let s2p = splitVersion(tyforce_string(s2));
            let ret = _.zip(s1p, s2p).map(([a,b]) => {
                if (a === b) return 0;
                const ina = a && a.match(/^[0-9]+$/g) !== null;
                const inb = b && b.match(/^[0-9]+$/g) !== null;
                if (ina && inb) {
                    const [pia,pib] = [parseInt(a),parseInt(b)];
                    return (pia < pib)?-1:((pia == pib)?0:1);
                }
                if ((a === '' || a === undefined) && inb) return -1;
                if (ina && (b === '' || b === undefined)) return 1;
                if (a === 'pre' || (!ina && inb)) return -1;
                if (b === 'pre' || (ina && !inb)) return 1;
                return (a < b)?-1:((a == b)?0:1);
            }).find(x => (x !== undefined) && (x !== 0));
            return (ret !== undefined) ? ret : 0;
        },
        concatLists: lists => tyforce_list(lists).flat(),
        concatMap: f => lists => tyforce_list(lists).map(f).flat(),
        concatStringsSep: sep => list => tyforce_list(list).join(tyforce_string(sep)),
        deepSeq: e1 => e2 => { deepSeq_helper(e1); return e2; },
        dirOf: s => {
            let tmp = tyforce_string(s).split('/');
            tmp.pop();
            return tmp.join('/');
        },
        div: a => b => {
            const bx = tyforce_number(b);
            // TODO: integer division?
            if (!bx) {
                throw RangeError('Division by zero');
            }
            return tyforce_number(a) / bx;
        },
        elem: x => xs => tyforce_list(xs).includes(x),
        elemAt: xs => n => {
            let tmp = tyforce_list(xs)[tyforce_number(n)];
            if (tmp === undefined) {
                throw RangeError('Index out of range');
            }
            return tmp;
        },

        // omitted: fetchGit, fetchTarball, fetchurl
        filter: f => list => tyforce_list(list).filter(f),
        // omitted: filterSource
        floor: n => Math.floor(tyforce_number(n)),
        "foldl'": op => nul => list => tyforce_list(list).reduce(force(op), force(nul)),
        fromJSON: e => anti_pollution(tyforce_string(e)),

        // TODO: functionArgs -- requires nix2js/lib.rs modification

        genList: gen_ => len => Array({length: tyforce_number(len)}, (dummy, i) => gen_(i)),
        getEnv: s => ((typeof process !== 'undefined') && (typeof process.env !== 'undefined'))
                ? process.env[tyforce_string(s)] : "",
        groupBy: f => list => _.groupBy(tyforce_list(list), force(f)),

        hasAttr: s => aset => Object.prototype.hasOwnProperty.call(force(aset), tyforce_string(s)),
        // omitted: hashFile, hashString
        head: list => {
            list = tyforce_list(list);
            if (!list.length) {
                throw RangeError('builtins.head called on empty list');
            }
            return list[0];
        },

        // omitted: import

        // ref: https://stackoverflow.com/a/1885569
        intersectAttrs: e1 => e2 => {
            let e2k = Object.keys(force(e2));
            // "value => ... includes(value)" is necessary to avoid TypeErrors
            return Object.keys(force(e1)).filter(value => e2k.includes(value)).filter(onlyUnique);
        },

        isAttrs: e => isAttrs(force(e)),
        isBool:  e => isBool(force(e)),
        isFloat: e => isNumber(force(e)),
        isFunction: e => force(e) instanceof Function,
        isInt:   e => typeof force(e) === 'bigint',
        isList:  e => force(e) instanceof Array,

        // DEPRECATED
        isNull:  e => force(e) === null,

        // TODO: isPath

        isString: e => isString(force(e)),

        length: e => tyforce_list(e).length,
        lessThan: e1 => e2 => tyforce_number(e1) < tyforce_number(e2),

        listToAttrs: list => fixObjectProto(
            Object.fromEntries(tyforce_list(list).map(ent => [ent.name, ent.value]))
        ),

        map: f => list => tyforce_list(list).map(force(f)),
        // ref: https://stackoverflow.com/a/14810722
        mapAttrs: f => aset => fixObjectProto(
            Object.fromEntries(Object.entries(force(aset)).map(([k, v]) => [k, f(k)(v)]))
        ),

        // TODO: `match`, maybe via compiling the original `prim_match` to webassembly

        mul: a => b => tyforce_number(a) * tyforce_number(b),

        parseDrvName: s => {
            let [name, version] = tyforce_string(s).split('-', 2);
            return fixObjectProto({ name, version });
        },
        partition: pred => list => {
            let [right, wrong] = _.partition(tyforce_list(list), force(pred));
            return fixObjectProto({ right, wrong });
        },

        // TODO: path, pathExists, placeholder
        // omitted: readDir, readFile

        removeAttrs: aset => list => {
            // make sure that we don't override the original object
            let aset2 = fixObjectProto(force(aset));
            for (const key of tyforce_list(list)) {
                delete aset2[key];
            }
            return aset2;
        },

        // ref: https://stackoverflow.com/a/67337940
        replaceStrings: from => to => s => {
            let entries = Object.entries(_.zip(from, to));
            return entries.reduce(
                    // Replace all the occurrences of the keys in the text into an index placholder using split-join
                    (_str, [key], i) => _str.split(key).join(`{${i}}`),
                    // Manipulate all exisitng index placeholder -like formats, in order to prevent confusion
                    s.replace(/\{(?=\d+\})/g, '{-')
                )
                // Replace all index placeholders to the desired replacement values
                .replace(/\{(\d+)\}/g, (_,i) => entries[i][1])
                // Undo the manipulation of index placeholder -like formats
                .replace(/\{-(?=\d+\})/g, '{');
        },

        seq: e1 => {
            if (e1 instanceof Lazy) {
                e1.evaluate();
            }
            return e2 => force(e2);
        },

        sort: comp => list => {
            let compx = force(comp);
            return tyforce_list(list).sort(a => b => {
                if (force(compx(a, b))) {
                    return -1;
                }
                if (force(compx(b, a))) {
                    return 1;
                }
                return 0;
            });
        },

        // TODO: `split`, see also: `match`

        splitVersion: s => splitVersion(tyforce_string(s)),

        // omitted: storePath

        stringLength: s => tyforce_string(s).length,

        tail: list => tyforce_list(list).slice(1),

        throw: s => {throw new NixEvalError(tyforce_string(s));},

        // TODO: toFile, via store interaction or derivation; weird stuff

        // TODO: handle derivations
        toJSON: x => JSON.stringify(force(x)),

        // omitted: toPath; also DEPRECATED

        // NOTE: we `force` in `nixToString`, because it recurses
        'toString': nixToString,

        // TODO: toXML

        trace: e1 => e2 => { console.debug(e1); return e2; },

        tryEval: e => {
            let success = false;
            let value = false;
            try {
                value = force(e);
                success = true;
            } catch(e) {
                if (!(typeof e === 'object' && e instanceof NixEvalError))
                    throw e;
                value = false;
                success = false;
            }
            return fixObjectProto({ value, success });
        },

        typeOf: e => {
            e = force(e);
            if (e === null) return "null";
            // need to differentiate this with `null` because of distinction via `isNull`,
            // and `isNull` deprecation.
            if (e === undefined) return "undefined";
            if (typeof e === 'object' && 'valueOf' in e)
                e = e.valueOf();
            let ety = typeof e;
            if (ety === 'object' && e instanceof Array)
                return "list";
            return nixTypeOf.hasOwnProperty(ety) ? nixTypeOf[ety] : ety;
        }
    };
}
