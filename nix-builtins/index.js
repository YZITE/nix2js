"use strict";

export const API_VERSION = 0;

export class Lazy {
    constructor(inner) {
        this.i = inner;
        this.iL = typeof inner === "function";
    }
    evaluate() {
        if (this.iL) {
            this.iL = false;
            let res = this.i.apply(this.i, arguments);
            this.i = res;
        }
        return this.i;
    }
    map(mapper) {
        return new Lazy(() => mapper(this.evaluate()));
    }
}

export function force(value) {
    if (value instanceof Lazy) {
        return value.evaluate();
    } else {
        return value;
    }
}

export function delay(value) {
    if (!(value instanceof Lazy)) {
        return Lazy(()=>value);
    } else {
        return value;
    }
}

export function inScope(orig, overlay) {
    if (overlay === undefined) {
        return orig;
    } else {
        return function(key, value) {
            let v1 = overlay(key, value);
            if (value !== undefined || v1 !== undefined) {
                return v1;
            } else {
                return orig(key, undefined);
            }
        };
    }
}

export function orDefault(lazy_selop, lazy_dfl) {
    let ret;
    try {
        ret = lazy_selop.evaluate();
    } catch (e) {
        if (e instanceof TypeError) {
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

export function initRtDep(nixRt) {
    return function(lineNo) {
        function binop_helper(name, f) {
            return function(a) {
                return function(c) {
                    let b = force(a);
                    let d = force(c);
                    let tb = typeof b;
                    let td = typeof d;
                    if (tb === td) {
                        return f(b, d);
                    } else {
                        nixRt.error("builtins." + name + ": given types mismatch (" + tb + " != " + td + ")", lineNo);
                    }
                };
            };
        }

        function req_number(opname, a) {
            if (typeof a !== 'number') {
                nixRt.error("builtins." + opname + ": invalid input type (" + typeof a + ")", lineNo);
            }
        }

        return {
            add: binop_helper("add", function(a, b) {
                req_number("add", a);
                return a + b;
            }),
            assert: function(lineNo, cond) {
                let cond2 = force(cond);
                if (typeof cond2 !== 'boolean') {
                    nixRt.error("assertion condition has wrong type (" + typeof cond2 + ")", lineNo);
                } else if (!cond2) {
                    nixRt.error("assertion failed", lineNo);
                }
            },
            nixop__Concat: binop_helper("++", function(a, b) {
                if (typeof a !== 'object') {
                    nixRt.error("builtins.++: invalid input type (" + typeof cond2 + ")", lineNo);
                }
                return a.concat(b);
            }),
            nixop__Add: binop_helper("+", function(a, b) {
                return a + b;
            }),
            nixop__Sub: binop_helper("-", function(a, b) {
                req_number("-", a);
                return a - b;
            }),
            nixop__Mul: binop_helper("*", function(a, b) {
                req_number("*", a);
                return a * b;
            })
        };
    };
}
