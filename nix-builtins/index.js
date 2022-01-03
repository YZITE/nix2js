// SPDX-License-Identifier: LGPL-2.1-or-later

import { isEqual } from 'lodash-es';

export const API_VERSION = 0;

const LazyHandler = {
    get: function(target, prop, receiver) {
        if (prop in target) {
            return target[prop];
        } else {
            return target.evaluate()[prop];
        }
    }
};

export class Lazy {
    constructor(inner) {
        this.i = inner;
        let toi = typeof inner;
        this.iL = toi === "function";
        if (toi === 'object') {
            // automatic unfolding
            return inner;
        } else {
            // automatic [] unfolding
            return new Proxy(this, LazyHandler);
        }
    }
    evaluate() {
        while (this.iL) {
            this.iL = false;
            let res = this.i.apply(this.i, arguments);
            // automatic unfolding
            if (res instanceof Lazy) {
                this.iL = res.iL;
                this.i = res.i;
            } else {
                this.i = res;
                break;
            }
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
    return new Lazy(()=>force(maker()));
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
            throw ReferenceError('dynscoped nix__' + key + ' is not defined');
        }
    };
}

export function mkScopeWith(orig, withns_) {
    return function(key, value) {
        let withns = force(withns_);
        if (key === undefined) {
            return Object.assign({}, withns);
        } else if (value !== undefined) {
            throw ReferenceError('withscoped nix__' + key + ' is read-only');
        } else if (withns[key] !== undefined) {
            return withns[key];
        } else if (orig !== undefined) {
            return orig(key, undefined);
        } else {
            throw ReferenceError('withscoped nix__' + key + ' is not defined');
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

function fmt_fname(fname) {
    if (fname.length != 1) {
        return fname;
    } else {
        return "operator " + fname;
    }
}

export function initRtDep(nixRt) {
    function binop_helper(fname, f) {
        return function(a, c) {
            let b = force(a);
            let d = force(c);
            let tb = typeof b;
            let td = typeof d;
            if (tb === td) {
                return f(b, d);
            } else {
                nixRt.throw(fmt_fname(fname) + ": given types mismatch (" + tb + " != " + td + ")");
            }
        };
    }

    function req_type(fname, x, xptype) {
        if (typeof x !== xptype) {
            nixRt.throw(fmt_fname(fname) + ": invalid input type (" + typeof x + "), expected (" + xptype + ")");
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
                    nixRt.throw("builtins.add: given types mismatch (" + tb + " != " + td + ")");
                }
            };
        },
        assert: function(cond) {
            let cond2 = force(cond);
            if (typeof cond2 !== 'boolean') {
                nixRt.throw("assertion condition has wrong type (" + typeof cond2 + ")");
            } else if (!cond2) {
                nixRt.throw("assertion failed");
            }
        },
        _deepMerge: function(attrs_, value, ...path) {
            let attrs = attrs_;
            while(1) {
                let pfi = path.shift();
                if (pfi === undefined) {
                    nixRt.throw("deepMerge: encountered empty path");
                    break;
                }
                if (typeof attrs !== 'object') {
                    nixRt.throw("deepMerge: tried to merge attrset into non-object (", attrs, ")");
                    break;
                }
                if (path.length) {
                    if (!attrs.hasOwnProperty(pfi)) {
                        attrs[pfi] = {};
                    }
                    attrs = attrs[pfi];
                } else {
                    attrs[pfi] = value;
                    break;
                }
            }
        },
        _lambdaA2chk: function(key, value) {
            if (value === undefined) {
                // TODO: adjust error message to what Nix currently issues.
                nixRt.throw("attrset element " + key + "missing at lambda call");
            }
            return value;
        },
        nixop__Concat: binop_helper("operator ++", function(a, b) {
            if (typeof a !== 'object') {
                nixRt.throw("operator ++: invalid input type (" + typeof a + ")");
            }
            return a.concat(b);
        }),
        // nixop__IsSet is implemented via .hasOwnProperty
        nixop__Update: binop_helper("operator //", function(a, b) {
            if (typeof a !== 'object') {
                nixRt.throw("operator //: invalid input type (" + typeof a + ")");
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
                nixRt.throw(fmt_fname("/") + ": division by zero");
            }
            return a / b;
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
        }),
        nixuop__Invert: function(a) {
            return !force(a);
        },
        nixuop__Negate: function(a) {
            return -force(a);
        }
    };
}
