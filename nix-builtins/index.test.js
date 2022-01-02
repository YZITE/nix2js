import { Lazy, force, mkScope, initRtDep } from "./index.js";
import isEqual from 'lodash-es';
import assert from 'webassert';

function mkMut(i) { return { i: i }; }
function assert_eq(a, b, msg) {
    if (!isEqual(a, b)) {
        console.warn("\tfor '" + msg + "': (" + a.toString() + ") !== (" + b.toString() + ")");
        assert(false, msg);
    }
}

class XpError {
    constructor(message, lno) {
        this.message = message;
        this.lno = lno;
    }
}

let instrum_blti = initRtDep(function(lno) {
    return {
        throw: function(msg) { throw new XpError(msg, lno); }
    };
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

    it('mappings should recurse', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert_eq(lobj.map(x => x + 1).evaluate(), 2, "indirect");
        assert_eq(lobj.evaluate(), 1, "secondary direct");
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
        let sc = mkScope();
        assert_eq(sc("a", 1), undefined, "(1s)");
        assert_eq(sc("a"), 1, "(1g)");
        assert_eq(sc("b", 2), undefined, "(2s)");
        assert_eq(sc("b"), 2, "(2g)");
        assert_eq(sc("a"), 1, "(3g)");
    });

    it('should work recursively', function() {
        let sc1 = mkScope();
        let sc2 = mkScope(sc1);
        assert_eq(sc1("a", 1), undefined, "(1)");
        assert_eq(sc2("a"), 1, "(2)");
        assert_eq(sc2("a", 2), undefined, "(3)");
        assert_eq(sc1("a"), 1, "(4)");
        assert_eq(sc2("a"), 2, "(5)");
    });
});

describe('add', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert_eq(blti(-1).add(1200)(567), 1767, "integer");
        assert_eq(blti(-2).add(-100)(567), 467, "integer (2)");
        assert_eq(blti(-3).add(203)(-500), -297, "integer (3)");
    });
    describe('should report errors correctly', function() {
        it("string/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(-4).add("ab")("cde"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add", "message");
                assert_eq(e.lno, -500, "lno");
            }
        });
        it("int/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(-500).add(0)("oops"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add: given types mismatch (number != string)", "message");
                assert_eq(e.lno, -500, "lno");
            }
        });
        it("string/int", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(275).add("oops")(0));
                assert(false, "unreachable");
            } catch(e) {
                assert(e instanceof XpError, "error kind");
                assert_eq(e.message, "builtins.add: given types mismatch (string != number)", "message");
                assert_eq(e.lno, 275, "lno");
            }
        });
    });
});

describe('+', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert_eq(blti(-1).nixop__Add(1200, 567), 1767, "integer");
        assert_eq(blti(-2).nixop__Add(-100, 567), 467, "integer (2)");
        assert_eq(blti(-3).nixop__Add(203, -500), -297, "integer (3)");
        assert_eq(blti(-4).nixop__Add("ab", "cde"), "abcde", "string");
    });
    describe('should report errors correctly', function() {
        it("int/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(-500).nixop__Add(0, "oops"));
                assert(false, "unreachable");
            } catch(e) {
                assert_eq(e.message, "operator +: given types mismatch (number != string)", "message");
                assert_eq(e.lno, -500, "lno");
            }
        });
        it("string/int", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(275).nixop__Add("oops", 0));
                assert(false, "unreachable");
            } catch(e) {
                assert_eq(e.message, "operator +: given types mismatch (string != number)", "message");
                assert_eq(e.lno, 275, "lno");
            }
        });
    });
});

it('-', function() {
    let blti = instrum_blti(0);
    assert_eq(blti.nixop__Sub(1200, 567), 633, "integer");
    assert_eq(blti.nixop__Sub(-100, 567), -667, "integer (2)");
    assert_eq(blti.nixop__Add(203, -500), 703, "integer (3)");
});

it('*', function() {
    let blti = instrum_blti(0);
    assert_eq(blti.nixop__Mul(50, 46), 2300, "integer");
    assert_eq(blti.nixop__Mul(50004, 1023), 51154092, "integer (2)");
    assert_eq(blti.nixop__Add(203, -500), -101500, "integer (3)");
    assert_eq(blti.nixop__Add(-203, 500), -101500, "integer (4)");
    assert_eq(blti.nixop__Add(-203, -500), 101500, "integer (4)");
});

describe('/', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti(0);
        assert_eq(blti.nixop__Div(1, 1), 1, "integer");
        assert_eq(blti.nixop__Div(8, 4), 2, "integer (2)");
        assert_eq(blti.nixop__Div(754677, 1331), 567, "integer (3)");
    });
    it('should catch division-by-zero', function() {
        let blti = instrum_blti;
        try {
            console.log(blti(277).nixop__Div(1, 0));
            assert(false, "unreachable");
        } catch(e) {
            assert(e instanceof XpError, "error kind");
            assert_eq(e.message, "operator /: division by zero", "message");
            assert_eq(e.lno, 277, "lno");
        }
    });
});

describe('//', function() {
    it('should merge distinct attrsets correctly', function() {
        let blti = instrum_blti(0);
        assert_eq(blti.nixop__Update({a: 1}, {b:2}), {a:1, b:2});
    });
    it('should merge overlapping attrsets correctly', function() {
        let blti = instrum_blti(0);
        let a = {a: {i: 0}};
        let b = {a: {i: 2}};
        assert_eq(blti.nixop__Update(a, b), {a: {i: 2}}, "//");
        assert_eq(a, {a: {i: 0}}, "original objects shouldn't be modified");
    });
});
