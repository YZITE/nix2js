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
    let value2 = value;
    if (value instanceof Lazy) {
        value2 = value.evaluate();
    }
    return value2;
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

export function initRtDep(nixRt) {
    return {
        add: function(lineNo, a) {
            return function(c) {
                let b = force(a);
                let d = force(c);
                let tb = typeof b;
                let td = typeof d;
                if (tb === td) {
                    return b + d;
                } else {
                    nixRt.error("builtins.add: given types mismatch (" + tb + " != " + td + ")", lineNo);
                }
            };
        },
        assert: function(lineNo, cond) {
            let cond2 = force(cond);
            if (typeof cond2 !== "boolean") {
                nixRt.error("assertion condition has wrong type " + typeof cond2, lineNo);
            } else if (!cond2) {
                nixRt.error("assertion failed", lineNo);
            }
        }
    };
}
