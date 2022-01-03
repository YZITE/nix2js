import { Lazy, force, initRtDep, allKeys, extractScope, mkScope, mkScopeWith } from "./index.js";
import { isEqual } from 'lodash-es';
import assert from 'webassert';

function mkMut(i) { return { i: i }; }
function assert_eq(a, b, msg) {
    if (!isEqual(a, b)) {
        console.warn("\tfor '" + msg + "': (", a, ") !== (", b, ")");
        assert(false, msg);
    }
}

class XpError {
    constructor(message) {
        this.message = message;
    }
}

let instrum_blti = initRtDep({
    throw: function(msg) { throw new XpError(msg); }
});

describe('Lazy', function() {
    it('should be lazy', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert_eq(lobj.evaluate(), 1, "1st");
        assert_eq(lobj.evaluate(), 1, "2nd");
        assert_eq(lobj.evaluate(), 1, "3rd");
    });

    it('should recurse/unfold', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return new Lazy(function() {
                ref.i += 1;
                return ref.i;
            });
        });
        assert_eq(lobj.evaluate(), 2, "1st");
        assert_eq(lobj.evaluate(), 2, "2nd");
        assert_eq(lobj.evaluate(), 2, "3rd");
    });

    it('automatic dereference should work', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return {
                a: 1,
                b: 2,
            };
        });
        assert_eq(ref.i, 0, "(0)");
        assert_eq(lobj['a'], 1, "(1)");
        assert_eq(ref.i, 1, "(1i)");
        assert_eq(lobj['b'], 2, "(2)");
        assert_eq(ref.i, 1, "(2i)");
        assert_eq(lobj['c'], undefined, "(3)");
        assert_eq(ref.i, 1, "(3i)");
    });
});

describe('force', function() {
    it('should work on Lazy', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert_eq(force(lobj), 1, "1st");
        assert_eq(force(lobj), 1, "2nd");
    });
    it('should work on primitives', function() {
        assert_eq(force(0), 0, "integer");
        assert_eq(force(0.0), 0.0, "float");
        assert_eq(force(""), "", "string");
        assert_eq(force("fshjdö"), "fshjdö", "string (2)");
    });
    it('shouldn\'t be tripped by iL in objects', function() {
        let tmpo = { iL: true };
        let tmpo2 = { iL: true };
        assert_eq(force(tmpo), tmpo2, "tripped by iL");
    });
});

describe('mkScope', function() {
    it('should work standalone', function() {
        let sc = mkScope(null);
        sc['a'] = 1;
        assert_eq(sc['a'], 1, "(1)");
        sc['b'] = 2;
        assert_eq(sc['b'], 2, "(2)");
        assert_eq(sc['a'], 1, "(3)");
        try {
            sc['a'] = 2;
            assert(false, "unreachable");
        } catch(e) {
            assert_eq(e.message, "'set' on proxy: trap returned falsish for property 'a'");
        }
        assert_eq(sc[allKeys], ['a', 'b']);
    });

    it('should work recursively', function() {
        let sc1 = mkScope(null);
        let sc2 = mkScope(sc1);
        sc1['a'] = 1;
        assert_eq(sc2['a'], 1, "(1)");
        sc2['a'] = 2;
        assert_eq(sc1['a'], 1, "(2)");
        assert_eq(sc2['a'], 2, "(3)");
        assert_eq(sc1[allKeys], ['a'], "(4)");
        assert_eq(sc2[allKeys], ['a'], "(5)");
        assert_eq(sc1[extractScope], {'a':1}, "(6)");
        assert_eq(sc2[extractScope], {'a':2}, "(7)");
    });
});

describe('mkScopeWith', function() {
    it('should deny modifications', function() {
        let sc = mkScopeWith();
        try {
            sc['a'] = 2;
            assert(false, "unreachable");
        } catch(e) {
            assert_eq(e.message, "tried overwriting key 'a' in read-only scope");
        }
    });

    it('should propagate get requests', function() {
        let scbase = mkScopeWith();
        let sc1 = mkScope(scbase);
        sc1['x'] = 1;
        let sc2 = mkScopeWith(sc1);
        try {
            sc2['x'] = 2;
            assert(false, "unreachable");
        } catch(e) {
            assert_eq(e.message, "tried overwriting key 'x' in read-only scope", "error message");
        }
        assert_eq(sc1[allKeys], ['x'], "(keys1)");
        assert_eq(sc2[allKeys], ['x'], "(keys2)");
        assert_eq(sc2['x'], 1, "(get)");
    });
});

describe('add', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert_eq(blti.add(1200)(567), 1767, "integer");
        assert_eq(blti.add(-100)(567), 467, "integer (2)");
        assert_eq(blti.add(203)(-500), -297, "integer (3)");
    });
    describe('should report errors correctly', function() {
        it("string/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti.add("ab")("cde"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add: invalid input type (string), expected (number)", "message");
            }
        });
        it("int/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti.add(0)("oops"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add: given types mismatch (number != string)", "message");
            }
        });
        it("string/int", function() {
            let blti = instrum_blti;
            try {
                console.log(blti.add("oops")(0));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add: given types mismatch (string != number)", "message");
            }
        });
    });
});

describe('+', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert_eq(blti.nixop__Add(1200, 567), 1767, "integer");
        assert_eq(blti.nixop__Add(-100, 567), 467, "integer (2)");
        assert_eq(blti.nixop__Add(203, -500), -297, "integer (3)");
        assert_eq(blti.nixop__Add("ab", "cde"), "abcde", "string");
    });
    describe('should report errors correctly', function() {
        it("int/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti.nixop__Add(0, "oops"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "operator +: given types mismatch (number != string)", "message");
            }
        });
        it("string/int", function() {
            let blti = instrum_blti;
            try {
                console.log(blti.nixop__Add("oops", 0));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "operator +: given types mismatch (string != number)", "message");
            }
        });
    });
});

it('-', function() {
    let blti = instrum_blti;
    assert_eq(blti.nixop__Sub(1200, 567), 633, "integer");
    assert_eq(blti.nixop__Sub(-100, 567), -667, "integer (2)");
    assert_eq(blti.nixop__Sub(203, -500), 703, "integer (3)");
});

it('*', function() {
    let blti = instrum_blti;
    assert_eq(blti.nixop__Mul(50, 46), 2300, "integer");
    assert_eq(blti.nixop__Mul(50004, 1023), 51154092, "integer (2)");
    assert_eq(blti.nixop__Mul(203, -500), -101500, "integer (3)");
    assert_eq(blti.nixop__Mul(-203, 500), -101500, "integer (4)");
    assert_eq(blti.nixop__Mul(-203, -500), 101500, "integer (5)");
});

describe('/', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert_eq(blti.nixop__Div(1, 1), 1, "integer");
        assert_eq(blti.nixop__Div(8, 4), 2, "integer (2)");
        assert_eq(blti.nixop__Div(754677, 1331), 567, "integer (3)");
    });
    it('should catch division-by-zero', function() {
        let blti = instrum_blti;
        try {
            console.log(blti.nixop__Div(1, 0));
            assert(false, "unreachable");
        } catch(e) {
            assert(e instanceof XpError, "error kind");
            assert_eq(e.message, "operator /: division by zero", "message");
        }
    });
});

describe('//', function() {
    it('should merge distinct attrsets correctly', function() {
        let blti = instrum_blti;
        assert_eq(blti.nixop__Update({a: 1}, {b:2}), {a:1, b:2});
    });
    it('should merge overlapping attrsets correctly', function() {
        let blti = instrum_blti;
        let a = {a: {i: 0}};
        let b = {a: {i: 2}};
        assert_eq(blti.nixop__Update(a, b), {a: {i: 2}}, "//");
        assert_eq(a, {a: {i: 0}}, "original objects shouldn't be modified");
    });
});

it('==', function() {
    assert_eq(instrum_blti.nixop__Equal(1, 1), true);
});

it('!=', function() {
    assert_eq(instrum_blti.nixop__NotEqual(1, 1), false);
});
