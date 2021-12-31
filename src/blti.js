// this code expects the variable 'nixRt' to be in-scope
let nixBlti = (function() {
    class NixLazy {
        constructor(inner) {
            this.inner = inner;
            this.isLazy = (typeof inner === 'function');
        }

        evaluate() {
            if(this.isLazy) {
                this.isLazy = false;
                let res = this.inner.apply(this.inner, arguments);
                this.inner = res;
            }
            return this.inner;
        }

        map(mapper) {
            return new NixLazy(() => mapper(this.evaluate()));
        }
    }

    function nix_force(value) {
        // force-evaluates a potentially-lazy value
        let value2 = value;
        if(value.isLazy === true) {
            value2 = value.evaluate();
        }
        return value2;
    }

    function nix_add(a) {
        return function(c) {
            let b = nix_force(a);
            let d = nix_force(c);

            if(typeof b === typeof d) {
                return b + d;
            } else {
                nixRt.error("builtins.add: given types mismatch");
            }
        }
    }

    function nix_assert(cond, fileName, lineNo) {
        let cond2 = nix_force(cond);
        if(typeof cond2 !== 'boolean') {
            nixRt.error("assertion condition has wrong type " + (typeof cond2), fileName, lineNo);
        } else if(!cond2) {
            nixRt.error("assertion failed", fileName, lineNo);
        }
    }

    function nix_in_scope(old_nis, add_nis, inner) {
        return inner(function(key) {
            if(add_nis === undefined) {
                return old_nis(key);
            }
            let v1 = add_nis(key);
            if(v1 !== undefined) {
                return v1;
            } else {
                return old_nis(key);
            }
        });
    }

    return {
        "Lazy": NixLazy,

        "assert": nix_assert,
        "force": nix_force,
        "in_scope": nix_in_scope,
    };
})();
let nixInScope = function(key) { return undefined; };
