import isEqual from 'lodash-es';

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

// this ensures correct evaluation when evaluating lazy values
export function mkLazy(maker) {
    return Lazy(()=>force(maker()));
}

export function delay(value) {
    if (!(value instanceof Lazy)) {
        return Lazy(()=>value);
    } else {
        return value;
    }
}

export function mkScope(orig) {
    // this is basically an interior mutable associative array.
    let scref = {i:{}};
    return function(key, value) {
        if (key === undefined) {
            return scref.i;
        } else if (value !== undefined) {
            scref.i[key] = value;
        } else if (scref.i[key] !== undefined) {
            return scref.i[key];
        } else if (orig !== undefined) {
            return orig(key, undefined);
        } else {
            throw ReferenceError('nix__' + key + ' is not defined');
        }
    };
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
        function fmt_fname(fname) {
            if (fname.length != 1) {
                return fname;
            } else {
                return "operator " + fname;
            }
        }

        function binop_helper(fname, f) {
            return function(a, c) {
                let b = force(a);
                let d = force(c);
                let tb = typeof b;
                let td = typeof d;
                if (tb === td) {
                    return f(b, d);
                } else {
                    nixRt.error(fmt_fname(fname) + ": given types mismatch (" + tb + " != " + td + ")", lineNo);
                }
            };
        }

        function req_type(fname, x, xptype) {
            if (typeof x !== xptype) {
                nixRt.error(fmt_fname(fname) + ": invalid input type (" + typeof x + "), expected (" + xptype + ")", lineNo);
            }
        }

        return {
            add: function(a) {
                return function(c) {
                    let b = force(a);
                    let d = force(c);
                    let tb = typeof b;
                    let td = typeof d;
                    if (tb === td) {
                        req_type("builtins.add", b, "number");
                        return b + d;
                    } else {
                        nixRt.error("builtins.add: given types mismatch (" + tb + " != " + td + ")", lineNo);
                    }
                };
            },
            assert: function(lineNo, cond) {
                let cond2 = force(cond);
                if (typeof cond2 !== 'boolean') {
                    nixRt.error("assertion condition has wrong type (" + typeof cond2 + ")", lineNo);
                } else if (!cond2) {
                    nixRt.error("assertion failed", lineNo);
                }
            },
            nixop__Concat: binop_helper("operator ++", function(a, b) {
                if (typeof a !== 'object') {
                    nixRt.error("operator ++: invalid input type (" + typeof a + ")", lineNo);
                }
                return a.concat(b);
            }),
            // nixop__IsSet is implemented via .hasOwnProperty
            nixop__Update: binop_helper("operator //", function(a, b) {
                if (typeof a !== 'object') {
                    nixRt.error("operator //: invalid input type (" + typeof a + ")", lineNo);
                }
                return Object.assign({}, a, b);
            }),
            nixop__Add: binop_helper("+", function(a, b) {
                return a + b;
            }),
            nixop__Sub: binop_helper("-", function(a, b) {
                req_type("-", a, "number");
                return a - b;
            }),
            nixop__Mul: binop_helper("*", function(a, b) {
                req_type("*", a, "number");
                return a * b;
            }),
            nixop__Div: binop_helper("/", function(a, b) {
                req_type("/", a, "number");
                if (!b) {
                    nixRt.error(fmt_fname("/") + ": division by zero", lineNo);
                }
                return a * b;
            }),
            nixop__And: binop_helper("&&", function(a, b) {
                req_type("&&", a, "boolean");
                return a && b;
            }),
            nixop__Implication: binop_helper("->", function(a, b) {
                req_type("->", a, "boolean");
                return (!a) || b;
            }),
            nixop__Or: binop_helper("||", function(a, b) {
                req_type("||", a, "boolean");
                return a || b;
            }),
            nixop__Equal: function(a, c) {
                let b = force(a);
                let d = force(c);
                return isEqual(b, d);
            },
            nixop__NotEqual: function(a, c) {
                let b = force(a);
                let d = force(c);
                return !isEqual(b, d);
            },
            nixop__Less: binop_helper("<", function(a, b) {
                req_type("<", a, "number");
                return a < b;
            }),
            nixop__LessOrEq: binop_helper("<=", function(a, b) {
                req_type("<=", a, "number");
                return a <= b;
            }),
            nixop__More: binop_helper(">", function(a, b) {
                req_type(">", a, "number");
                return a > b;
            }),
            nixop__MoreOrEq: binop_helper(">=", function(a, b) {
                req_type(">=", a, "number");
                return a >= b;
            })
        };
    };
}
