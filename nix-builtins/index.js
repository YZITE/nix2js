// SPDX-License-Identifier: LGPL-2.1-or-later

import { isEqual } from 'lodash-es';

export const API_VERSION = 0;

const LazyHandler = {
    get: (target, key) => (key in target) ? target[key] : target.evaluate()[key],
    has: (target, key) => (key in target) || (key in target.evaluate()),
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
}

export function force(value) {
    if (value instanceof Lazy) {
        return value.evaluate();
    } else {
        return value;
    }
}

function onlyUnique(value, index, self) {
    return self.indexOf(value) == index;
}

class ScopeError extends Error { constructor(message, options) { super(message, options); } }

// used to get all keys present in a scope, including inherited ones
export const allKeys = Symbol('__all__');

// used to get the current scope, but detached from it's parent scope and
// without the proxy wrapper.
export const extractScope = Symbol('__dict__');

export function mkScope(orig) {
    // we need to handle mkScope()
    let orig_keys = orig ? (() => Object.keys(orig)) : (() => []);
    // Object.create prevents prototype pollution
    let current = Object.create(orig);
    // self-referential properties
    Object.defineProperty(current, allKeys, {
        get:()=>Object.keys(current).concat(orig_keys()).filter(onlyUnique),
    });
    Object.defineProperty(current, extractScope, {
        get:()=>Object.assign(Object.create(null),current),
    });
    return new Proxy(current, {
        set: function(target, key, value) {
            let ret = !Object.prototype.hasOwnProperty.call(target, key);
            if (ret) {
                Object.defineProperty(target, key, {
                    value: value,
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
        throw new ScopeError("tried overwriting key '" + key + "' in read-only scope");
    },
    deleteProperty: function(target, key) {
        if (key in target) {
            throw new ScopeError("tried removing key '" + key + "' from read-only scope");
        } else {
            return false;
        }
    },
    defineProperty: (target, key, desc) => {
        throw new ScopeError("tried modifying property '" + key + "' on read-only scope");
    },
    getOwnPropertyDescriptor: function(target, key) {
        let tmp = Object.getOwnPropertyDescriptor(target, key);
        if (tmp) {
            tmp.writable = false;
        }
        return tmp;
    }
};

export function mkScopeWith(...objs) {
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

    return [{
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
        }
    },
    {
        u_Invert: a => !force(a),
        u_Negate: a => -force(a),
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
        _lambdaA2chk: function(attrs, key) {
            if (attrs[key] === undefined) {
                // TODO: adjust error message to what Nix currently issues.
                nixRt.throw("attrset element " + key + "missing at lambda call");
            }
            return attrs[key];
        },
        Concat: binop_helper("operator ++", function(a, b) {
            if (typeof a !== 'object') {
                nixRt.throw("operator ++: invalid input type (" + typeof a + ")");
            }
            return a.concat(b);
        }),
        // IsSet is implemented via .hasOwnProperty
        Update: binop_helper("operator //", function(a, b) {
            if (typeof a !== 'object') {
                nixRt.throw("operator //: invalid input type (" + typeof a + ")");
            }
            return Object.assign({}, a, b);
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
                nixRt.throw(fmt_fname("/") + ": division by zero");
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
        Equal: function(a, c) {
            let b = force(a);
            let d = force(c);
            return isEqual(b, d);
        },
        NotEqual: function(a, c) {
            let b = force(a);
            let d = force(c);
            return !isEqual(b, d);
        },
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
    }];
}
