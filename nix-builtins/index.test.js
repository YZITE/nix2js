import { Lazy, force, inScope, initRtDep } from "./index.js";
import isEqual from 'lodash-es';
import assert from 'webassert';

let mkMut = function(i) { return { i: i }; };

class XpError {
    constructor(message, lno) {
        this.message = message;
        this.lno = lno;
    }
}

let instrum_blti = initRtDep({
    error: function(msg, lno) { throw new XpError(msg, lno); }
});

describe('Lazy', function() {
    it('should be lazy', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert(lobj.evaluate() === 1, "1st");
        assert(lobj.evaluate() === 1, "2nd");
        assert(lobj.evaluate() === 1, "3rd");
    });

    it('mappings should recurse', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert(lobj.map(x => x + 1).evaluate() === 2, "indirect");
        assert(lobj.evaluate() === 1, "secondary direct");
    });
});

describe('force', function() {
    it('should work on Lazy', function() {
        let ref = mkMut(0);
        let lobj = new Lazy(function() {
            ref.i += 1;
            return ref.i;
        });
        assert(force(lobj) === 1, "1st");
        assert(force(lobj) === 1, "2nd");
    });
    it('should work on primitives', function() {
        assert(force(0) === 0, "integer");
        assert(force(0.0) === 0.0, "float");
        assert(force("") === "", "string");
        assert(force("fshjdö") === "fshjdö", "string (2)");
    });
    it('shouldn\'t be tripped by iL in objects', function() {
        let tmpo = { iL: true };
        let tmpo2 = { iL: true };
        assert(isEqual(force(tmpo), tmpo2));
    });
});

describe('add', function() {
    it('should work if arguments are correct', function() {
        let blti = instrum_blti;
        assert(blti(-1).add(1200)(567) === 1767, "integer");
        assert(blti(-2).add(-100)(567) === 467, "integer (2)");
        assert(blti(-3).add(203)(-500) === -297, "integer (3)");
        assert(blti(-4).add("ab")("cde") === "abcde", "string");
    });
    describe('should report errors correctly', function() {
        it("int/string", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(-500).add(0)("oops"));
                assert(false, "unreachable");
            } catch(e) {
                assert(e.message === "builtins.add: given types mismatch (number != string)", "message");
                assert(e.lno === -500, "lno");
            }
        });
        it("string/int", function() {
            let blti = instrum_blti;
            try {
                console.log(blti(275).add("oops")(0));
                assert(false, "unreachable");
            } catch(e) {
                assert(e.message === "builtins.add: given types mismatch (string != number)", "message");
                assert(e.lno === 275, "lno");
            }
        });
    });
});
