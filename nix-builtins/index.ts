// SPDX-License-Identifier: LGPL-2.1-or-later

import * as _ from "lodash-es";
import assert from "webassert";
import PLazy from "p-lazy";
export { default as PLazy } from "p-lazy";

export const API_VERSION = 0;

export class NixAbortError extends Error {}
export class NixEvalError extends Error {}

// TODO: add class for StringWithContext, although that might be unnecessary,
// because we don't serialize derivations before submit...

type MaybePromise<T> = T | Promise<T>;

const onlyUnique = (value, index, self) => self.indexOf(value) == index;
const fmt_fname = (fname: string): string =>
  fname.length != 1 ? fname : "operator " + fname;
const natyforce = (objty, natty, ax) =>
  function (val) {
    if (val instanceof objty) {
      val = val.valueOf();
    }
    if (typeof val !== natty) {
      throw TypeError(
        "value is " +
          typeof val +
          " while " +
          ax +
          " " +
          natty +
          " was expected"
      );
    }
    return val;
  };
const otyforce = (objty, ax) =>
  function (val) {
    if (typeof val !== "object") {
      throw TypeError(
        "value is " + typeof val + " while an object was expected"
      );
    }
    if (!(val instanceof objty)) {
      throw TypeError(
        "value is " +
          val.constructor.name +
          " while " +
          ax +
          " " +
          objty.constructor.name +
          " was expected"
      );
    }
    return val;
  };
const discardStringContext = (s) => s;
const tyforce_string = natyforce(String, "string", "a");
const tyforce_number = (val) =>
  typeof val === "bigint" ? val : natyforce(Number, "number", "a")(val);
const tyforce_list = otyforce(Array, "an");

const isnaty = (objty, natty) => (val) =>
  val instanceof objty || typeof val === natty;
const isBool = isnaty(Boolean, "boolean");
const isNumber = isnaty(Number, "number");
const isString = isnaty(String, "string");

// the assignment ensures that future assignments won't currupt the prototype
export const fixObjectProto = (...objs) =>
  Object.assign(Object.create(null), ...objs);

export class ScopeError extends Error {}

// used to get all keys present in a scope, including inherited ones
export const allKeys = Symbol("__all__");

// used to get the current scope, but detached from it's parent scope and
// without the proxy wrapper.
export const extractScope = Symbol("__dict__");

export function mkScope(orig?: null | object): object {
  if (orig === undefined) {
    // "Object prototype may only be an Object or null"
    orig = null;
  }
  // we need to handle mkScope()
  let orig_keys = orig ? () => Object.keys(orig) : () => [];
  // Object.create prevents prototype pollution
  let current = Object.create(orig);
  // self-referential properties
  Object.defineProperty(current, allKeys, {
    get: () => Object.keys(current).concat(orig_keys()).filter(onlyUnique),
  });
  Object.defineProperty(current, extractScope, {
    get: () => fixObjectProto(current),
  });
  return new Proxy(current, {
    set: function (target, key, value) {
      if (key == "__proto__") throw new ScopeError("Tried modifying prototype");
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
    },
  });
}

// mark an object as read-only
const readOnlyHandler = {
  set: function (target, key, value) {
    throw new ScopeError(
      "Tried overwriting key '" + key + "' in read-only scope"
    );
  },
  deleteProperty: function (target, key) {
    if (key in target) {
      throw new ScopeError(
        "Tried removing key '" + key + "' from read-only scope"
      );
    } else {
      return false;
    }
  },
  defineProperty: (target, key, desc) => {
    throw new ScopeError(
      "Tried modifying property '" + key + "' on read-only scope"
    );
  },
  getOwnPropertyDescriptor: function (target, key) {
    let tmp = Object.getOwnPropertyDescriptor(target, key);
    if (tmp) {
      tmp.writable = false;
    }
    return tmp;
  },
};

export function mkScopeWith(...objs: object[]): object {
  let handler = Object.create(readOnlyHandler);
  handler.get = (target, key) => {
    if (key in target) {
      return target[key];
    } else {
      let tmp = objs.find((obj) => key in obj);
      return tmp !== undefined ? tmp[key] : undefined;
    }
  };
  handler.has = (target, key) =>
    key in target || objs.some((obj) => key in obj);
  return new Proxy(
    Object.create(null, {
      [allKeys]: {
        get: () =>
          objs
            .map((obj) => obj[allKeys])
            .flat()
            .filter((i) => i !== undefined)
            .filter(onlyUnique),
      },
    }),
    handler
  );
}

const splitVersion = (s) =>
  s
    .split(/[^A-Za-z0-9]/)
    .map((x) => x.split(/([A-Za-z]+|[0-9]+)/).filter((elem, idx) => idx % 2))
    .flat();

export async function orDefault<T>(
  selopf: T | PLazy<T>,
  dflf: T | PLazy<T>
): Promise<T> {
  let ret = undefined;
  try {
    ret = await selopf;
  } catch (e) {
    // this is flaky...
    if (
      e instanceof TypeError &&
      e.message.startsWith("Cannot read properties of undefined ")
    ) {
      console.debug("nix-blti.orDefault: encountered+catched TypeError:", e);
    } else {
      console.debug("nix-blti.orDefault: encountered+forwarded:", e);
      throw e;
    }
  }
  if (ret === undefined) {
    ret = await dflf;
  }
  return ret;
}

function binop_helper<T, R>(fname: string, f: (a: T, b: T) => R) {
  return async function (a: MaybePromise<T>, b: MaybePromise<T>): Promise<R> {
    a = await a;
    b = await b;
    let ta = typeof a;
    let tb = typeof b;
    if (ta === tb) {
      return f(a, b);
    } else {
      throw TypeError(
        fmt_fname(fname) + ": given types mismatch (" + ta + " != " + tb + ")"
      );
    }
  };
}

function req_type<T>(fname: string, x: T, xptype: string): void {
  if (typeof x !== xptype) {
    throw TypeError(
      fmt_fname(fname) +
        ": invalid input type (" +
        typeof x +
        "), expected (" +
        xptype +
        ")"
    );
  }
}

function req_number<T>(fname: string, x: T, y: T): [number, number] {
  req_type(fname, x, "number");
  return [x as any as number, y as any as number];
}

function req_boolean<T>(fname: string, x: T, y: T): [boolean, boolean] {
  req_type(fname, x, "boolean");
  return [x as any as boolean, y as any as boolean];
}

const isAttrs = (e: any): boolean =>
  typeof e === "object" &&
  !(e instanceof Boolean || e instanceof Number || e instanceof String);

const deepSeq_helper = async (e) => {
  e = await e;
  if (isAttrs(e)) {
    await Promise.all(e.map((i) => deepSeq_helper(i)));
  }
};

// async list helpers

export type MaybePromiseList<T> = MaybePromise<MaybePromise<T>[]>;

export let resolveList = async <T>(list: MaybePromiseList<T>): Promise<T[]> =>
  await Promise.all(await list);

async function transformAsyncList<I, R>(
  list: any,
  befres: (l: any[]) => MaybePromise<I>[],
  aftres: (l: I[]) => R[]
): Promise<R[]> {
  return aftres(await Promise.all(befres(tyforce_list(await list))));
}

export async function filterAsyncList<T>(
  list: MaybePromiseList<T>,
  f: (x: T) => MaybePromise<boolean>
): Promise<T[]> {
  let list2: Promise<[T, boolean]>[] = (await list).map(
    async (x: MaybePromise<T>): Promise<[T, boolean]> => {
      // we need to await x here because the function f is expected
      // to be pure and we don't want to waste any energy with
      // settled Promise's.
      let y = await x;
      return [y, await f(y)];
    }
  );
  return (await Promise.all(list2)).reduce((acc: T[], z: [T, boolean]): T[] => {
    if (z[1]) acc.push(z[0]);
    return acc;
  }, []);
}

export async function sortAsyncList<T>(
  list: MaybePromiseList<T>,
  comp: (a: T, b: T) => MaybePromise<boolean>
): Promise<T[]> {
  comp = await comp;
  let list_: T[] = await Promise.all(tyforce_list(await list));
  // precompute comparator results
  let mtx: number[][] = await Promise.all(
    list_.map(
      async (a, ax) =>
        await Promise.all(
          list_
            .filter((dummy, bx) => bx < ax)
            .map(async (b) => {
              if (await comp(a, b)) {
                return -1;
              } else if (await comp(b, a)) {
                return 1;
              } else {
                return 0;
              }
            })
        )
    )
  );
  // apply them
  return list_
    .map((i, ix): [T, number] => [i, ix])
    .sort(([a, ax], [b, bx]) => (bx < ax ? mtx[ax][bx] : -mtx[bx][ax]))
    .map(([i, ix]: [T, number]): T => i);
}

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
const anti_pollution = (e: string): object =>
  fixObjectProto(
    JSON.parse(
      e.replace(
        new RegExp(
          "(_|\\\\_|[\\\\u05fFx]+){2}proto(_|\\\\_|[\\\\u05fFx]+){2}(?=[\"']+[\\s:]+[{\"'])",
          "g"
        ),
        "__pollutants__"
      )
    )
  );

const nixToStringHandler = {
  object: async function (x: object): Promise<string> {
    // TODO: handle paths
    // "A list, in which case the string representations of its elements are joined with spaces."
    if (x instanceof Array)
      return (await Promise.all(x.map(nixToString))).join(" ");
    if ("toString" in x) return x.toString();
    if ("__toString" in x) return x["__toString"](x);
    if ("outPath" in x) return x["outPath"];
    throw new NixEvalError(
      "nixToString: unserializable object type " + x.constructor.name
    );
  },
  string: (x) => x,
  bigint: (x) => x.toString(),
  number: (x) => x.toString(),
  boolean: (x) => (x ? "1" : ""),
};

async function nixToString(x: any): Promise<string> {
  x = await x;
  if (x === null || x === undefined) return "";
  if (typeof x === "object" && "valueOf" in x) x = x.valueOf();
  if (nixToStringHandler.hasOwnProperty(typeof x)) {
    return await nixToStringHandler[typeof x](x);
  }
  throw new NixEvalError("nixToString: unserializable type " + typeof x);
}

const nixTypeOf = {
  bigint: "int",
  boolean: "bool",
  function: "lambda",
  number: "float",
  object: "set",
};

// operators
export const nixOp = {
  u_Invert: async (a) => !(await a),
  u_Negate: async (a) => -(await a),
  _deepMerge: async function (
    attrs_: object | Promise<object>,
    value: any,
    ...path: string[]
  ): Promise<void> {
    let attrs = await attrs_;
    while (1) {
      let pfi = path.shift();
      if (pfi === undefined) {
        throw new NixEvalError("deepMerge: encountered empty path");
      }
      if (typeof attrs !== "object") {
        throw new NixEvalError(
          "deepMerge: tried to merge attrset into non-object (" +
            typeof attrs +
            ")"
        );
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
  _lambdaA2chk: async function (
    attrs: object,
    key: string,
    fallback?: Promise<any>
  ): Promise<any> {
    let tmp = await attrs[key];
    if (tmp === undefined) {
      if (fallback === undefined) {
        // TODO: adjust error message to what Nix currently issues.
        throw new NixEvalError(
          "Attrset element " + key + "missing at lambda call"
        );
      } else {
        tmp = await fallback;
      }
    }
    return tmp;
  },
  Concat: binop_helper("operator ++", function (a: any[], b: any[]) {
    if (typeof a !== "object") {
      throw TypeError("operator ++: invalid input type (" + typeof a + ")");
    }
    return a.concat(b);
  }),
  // IsSet is implemented via .hasOwnProperty
  Update: binop_helper("operator //", function (a: object, b: object) {
    if (typeof a !== "object") {
      throw TypeError("operator //: invalid input type (" + typeof a + ")");
    }
    return fixObjectProto({}, a, b);
  }),
  Add: binop_helper("+", function <T>(a: T, b: T) {
    if (typeof a === "number") {
      return a + (b as any as number);
    } else if (typeof a === "string") {
      return a + (b as any as string);
    } else {
      throw TypeError("operator +: invalid input type (" + typeof a + ")");
    }
  }),
  Sub: binop_helper("-", function <T>(a: T, b: T) {
    let [c, d] = req_number("-", a, b);
    return c - d;
  }),
  Mul: binop_helper("*", function <T>(a: T, b: T) {
    let [c, d] = req_number("*", a, b);
    return c * d;
  }),
  Div: binop_helper("/", function <T>(a: T, b: T) {
    let [c, d] = req_number("/", a, b);
    if (!d) {
      throw RangeError("Division by zero");
    }
    return c / d;
  }),
  And: binop_helper("&&", function <T>(a: T, b: T) {
    let [c, d] = req_boolean("&&", a, b);
    return c && d;
  }),
  Implication: binop_helper("->", function <T>(a: T, b: T) {
    req_boolean("->", a, b);
    return !a || b;
  }),
  Or: binop_helper("||", function <T>(a: T, b: T) {
    req_boolean("||", a, b);
    return a || b;
  }),
  Equal: async (a, b) => _.isEqual(await a, await b),
  NotEqual: async (a, b) => !_.isEqual(await a, await b),
  Less: binop_helper("<", function <T>(a: T, b: T) {
    req_number("<", a, b);
    return a < b;
  }),
  LessOrEq: binop_helper("<=", function <T>(a: T, b: T) {
    req_number("<=", a, b);
    return a <= b;
  }),
  More: binop_helper(">", function <T>(a: T, b: T) {
    req_number(">", a, b);
    return a > b;
  }),
  MoreOrEq: binop_helper(">=", function <T>(a: T, b: T) {
    req_number(">=", a, b);
    return a >= b;
  }),
};

export function initRtDep(nixRt) {
  return {
    abort: async (s) => {
      throw new NixAbortError(tyforce_string(await s));
    },
    add: (a) =>
      async function (b) {
        a = await a;
        b = await b;
        let ta = typeof a;
        let tb = typeof b;
        if (ta !== tb) {
          throw TypeError(
            "builtins.add: given types mismatch (" + ta + " != " + tb + ")"
          );
        } else if (ta === "number") {
          return a + b;
        } else {
          throw TypeError(
            "builtins.add: invalid input type (" + ta + "), expected (number)"
          );
        }
      },
    all: (pred) => async (list) =>
      (await Promise.all(tyforce_list(await list).map(pred))).every((x) => x),
    any: (pred) => async (list) =>
      (await Promise.all(tyforce_list(await list).map(pred))).some((x) => x),
    assert: (condstr: string) => async (cond) => {
      if (typeof cond === "function") {
        // async functions are still functions
        cond = cond();
      }
      const cond2 = await cond;
      if (typeof cond2 !== "boolean") {
        throw TypeError(
          "Assertion condition has wrong type (" + typeof cond2 + ")"
        );
      }
      assert(cond2, condstr);
    },
    attrNames: async (aset) => Object.keys(await aset).sort(),
    attrValues: async (aset) =>
      Object.entries(await aset)
        .sort()
        .map((a) => a[1]),
    baseNameOf: async (s) => _.last(tyforce_string(await s).split("/")),
    bitAnd: (v1) => async (v2) =>
      tyforce_number(await v1) & tyforce_number(await v2),
    bitOr: (v1) => async (v2) =>
      tyforce_number(await v1) | tyforce_number(await v2),
    catAttrs: (s) => async (list) => {
      const s2 = tyforce_string(await s);
      return (await resolveList(tyforce_list(await list)))
        .filter((aset) => Object.prototype.hasOwnProperty.call(aset, s2))
        .map((aset) => aset[s2]);
    },
    ceil: async (n) => Math.ceil(tyforce_number(await n)),
    compareVersions: (s1) => async (s2) => {
      let s1p = splitVersion(tyforce_string(await s1));
      let s2p = splitVersion(tyforce_string(await s2));
      let ret = _.zip(s1p, s2p)
        .map(([a, b]) => {
          if (a === b) return 0;
          const ina = a && a.match(/^[0-9]+$/g) !== null;
          const inb = b && b.match(/^[0-9]+$/g) !== null;
          if (ina && inb) {
            const [pia, pib] = [parseInt(a), parseInt(b)];
            return pia < pib ? -1 : pia == pib ? 0 : 1;
          }
          if ((a === "" || a === undefined) && inb) return -1;
          if (ina && (b === "" || b === undefined)) return 1;
          if (a === "pre" || (!ina && inb)) return -1;
          if (b === "pre" || (ina && !inb)) return 1;
          return a < b ? -1 : a == b ? 0 : 1;
        })
        .find((x) => x !== undefined && x !== 0);
      return ret !== undefined ? ret : 0;
    },
    concatLists: async (lists) =>
      await transformAsyncList(
        lists,
        (x) => x,
        (x) => x.flat()
      ),
    concatMap: (f) => async (lists) =>
      await transformAsyncList(
        lists,
        (x) => x.map(f),
        (x) => x.flat()
      ),
    concatStringsSep: (sep) => async (list) =>
      (await resolveList(tyforce_list(await list))).join(
        tyforce_string(await sep)
      ),
    deepSeq: async (e1) => {
      await deepSeq_helper(e1);
      return (e2) => e2;
    },
    dirOf: async (s) => {
      let tmp = tyforce_string(await s).split("/");
      tmp.pop();
      return tmp.join("/");
    },
    div: (a) => async (b) => {
      const bx = tyforce_number(await b);
      // TODO: integer division?
      if (!bx) {
        throw RangeError("Division by zero");
      }
      return tyforce_number(await a) / bx;
    },
    elem: (x) => async (xs) =>
      (await Promise.all(tyforce_list(await xs))).includes(await x),
    elemAt: (xs) => async (n) => {
      let tmp = await tyforce_list(await xs)[tyforce_number(await n)];
      if (tmp === undefined) {
        throw RangeError("Index out of range");
      }
      return tmp;
    },

    // omitted: fetchGit, fetchTarball, fetchurl
    filter: (f) => async (list) =>
      await filterAsyncList(tyforce_list(await list), await f),
    // omitted: filterSource
    floor: async (n) => Math.floor(tyforce_number(await n)),
    "foldl'": (op) => (nul) => async (list) =>
      tyforce_list(await list).reduce(await op, nul),
    fromJSON: async (e) => anti_pollution(tyforce_string(await e)),

    // TODO: functionArgs -- requires nix2js/lib.rs modification

    genList: (gen_) => async (len) =>
      Array({ length: tyforce_number(await len) }, (dummy, i) => gen_(i)),
    getEnv: async (s) =>
      typeof process !== "undefined" && typeof process.env !== "undefined"
        ? process.env[tyforce_string(await s)]
        : "",
    groupBy: (f) => async (list) =>
      _.groupBy(tyforce_list(await list), await f),

    hasAttr: (s) => async (aset) =>
      Object.prototype.hasOwnProperty.call(await aset, tyforce_string(await s)),
    // omitted: hashFile, hashString
    head: async (list) => {
      list = tyforce_list(await list);
      if (!list.length) {
        throw RangeError("builtins.head called on empty list");
      }
      return list[0];
    },

    // omitted: import

    // ref: https://stackoverflow.com/a/1885569
    intersectAttrs: (e1) => async (e2) => {
      let e2k = Object.keys(await e2);
      // "value => ... includes(value)" is necessary to avoid TypeErrors
      return Object.keys(await e1)
        .filter((value) => e2k.includes(value))
        .filter(onlyUnique);
    },

    isAttrs: async (e) => isAttrs(await e),
    isBool: async (e) => isBool(await e),
    isFloat: async (e) => isNumber(await e),
    isFunction: async (e) => (await e) instanceof Function,
    isInt: async (e) => typeof (await e) === "bigint",
    isList: async (e) => (await e) instanceof Array,

    // DEPRECATED
    isNull: async (e) => (await e) === null,

    // TODO: isPath

    isString: async (e) => isString(await e),

    length: async (e) => tyforce_list(await e).length,
    lessThan: (e1) => async (e2) =>
      tyforce_number(await e1) < tyforce_number(await e2),

    listToAttrs: async (list) =>
      fixObjectProto(
        Object.fromEntries(
          await Promise.all(
            tyforce_list(await list).map(async (ent) => {
              ent = await ent;
              return [ent.name, ent.value];
            })
          )
        )
      ),

    map: (f) => async (list) => tyforce_list(await list).map(await f),
    // ref: https://stackoverflow.com/a/14810722
    mapAttrs: (f) => async (aset: MaybePromise<object>) =>
      fixObjectProto(
        Object.fromEntries(
          Object.entries(await aset).map(([k, v]) => [
            k,
            (async (k_, v_) => await (await f(k))(v))(k, v),
          ])
        )
      ),

    // TODO: `match`, maybe via compiling the original `prim_match` to webassembly

    mul: (a) => async (b) => tyforce_number(await a) * tyforce_number(await b),

    parseDrvName: async (s) => {
      let [name, version] = tyforce_string(await s).split("-", 2);
      return fixObjectProto({ name, version });
    },
    partition: (pred) => async (list) => {
      // no need to resolve the list, the predicate can handle that
      let [right, wrong] = _.partition(tyforce_list(await list), await pred);
      return fixObjectProto({ right, wrong });
    },

    // TODO: path, pathExists, placeholder
    // omitted: readDir, readFile

    removeAttrs: (aset) => async (list) => {
      // make sure that we don't override the original object
      let aset2 = fixObjectProto(await aset);
      for (const key of tyforce_list(await list)) {
        delete aset2[await key];
      }
      return aset2;
    },

    // ref: https://stackoverflow.com/a/67337940
    replaceStrings: (from_) => (to) => async (s) => {
      let from__ = await resolveList(tyforce_list(from_));
      let to__ = await resolveList(tyforce_list(to));
      let entries = Object.entries(_.zip(from_, to));
      return (
        entries
          .reduce(
            // Replace all the occurrences of the keys in the text into an index placholder using split-join
            (_str, [key], i) => _str.split(key).join(`{${i}}`),
            // Manipulate all exisitng index placeholder -like formats, in order to prevent confusion
            (await s).replace(/\{(?=\d+\})/g, "{-")
          )
          // Replace all index placeholders to the desired replacement values
          .replace(/\{(\d+)\}/g, (_, i) => entries[i][1])
          // Undo the manipulation of index placeholder -like formats
          .replace(/\{-(?=\d+\})/g, "{")
      );
    },

    seq: async (e1) => {
      await e1;
      return (e2) => e2;
    },

    sort: (comp) => async (list) => sortAsyncList(list, await comp),

    // TODO: `split`, see also: `match`

    splitVersion: async (s) => splitVersion(tyforce_string(await s)),

    // omitted: storePath

    stringLength: async (s) => tyforce_string(await s).length,

    tail: async (list) => tyforce_list(await list).slice(1),

    throw: async (s) => {
      throw new NixEvalError(tyforce_string(await s));
    },

    // TODO: toFile, via store interaction or derivation; weird stuff

    // TODO: handle derivations
    toJSON: async (x) => JSON.stringify(await x),

    // omitted: toPath; also DEPRECATED

    // NOTE: we `await` in `nixToString`, because it recurses
    toString: nixToString,

    // TODO: toXML

    trace: (e1) => (e2) => {
      console.debug(e1);
      return e2;
    },

    tryEval: async (e) => {
      let success = false;
      let value = false;
      try {
        value = await e;
        success = true;
      } catch (e) {
        if (!(typeof e === "object" && e instanceof NixEvalError)) throw e;
        value = false;
        success = false;
      }
      return fixObjectProto({ value, success });
    },

    typeOf: async (e) => {
      e = await e;
      if (e === null) return "null";
      // need to differentiate this with `null` because of distinction via `isNull`,
      // and `isNull` deprecation.
      if (e === undefined) return "undefined";
      if (typeof e === "object" && "valueOf" in e) e = e.valueOf();
      let ety = typeof e;
      if (ety === "object" && e instanceof Array) return "list";
      return nixTypeOf.hasOwnProperty(ety) ? nixTypeOf[ety] : ety;
    },
  };
}
