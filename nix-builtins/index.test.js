import {
  allKeys,
  extractScope,
  initRtDep,
  mkScope,
  mkScopeWith,
  NixEvalError,
  nixOp,
  PLazy,
  ScopeError,
} from "./index.js";
import { isEqual } from "lodash-es";
import assert from "webassert";

function mkMut(i) {
  return { i: i };
}
function assert_eq(a, b, msg) {
  if (!isEqual(a, b)) {
    console.warn("\tfor '" + msg + "': (", a, ") !== (", b, ")");
    assert(false, msg);
  }
}

let xblti = initRtDep({});

describe("mkScope", function () {
  it("should work standalone", function () {
    let sc = mkScope(null);
    sc["a"] = 1;
    assert_eq(sc["a"], 1, "(1)");
    sc["b"] = 2;
    assert_eq(sc["b"], 2, "(2)");
    assert_eq(sc["a"], 1, "(3)");
    try {
      sc["a"] = 2;
      assert(false, "unreachable");
    } catch (e) {
      assert_eq(
        e.message,
        "'set' on proxy: trap returned falsish for property 'a'"
      );
    }
    assert_eq(sc[allKeys], ["a", "b"]);
  });

  it("shouldn't allow direct modifications of prototype", function () {
    let a = mkScope(null);
    let sc = mkScope(a);
    try {
      assert_eq(sc["__proto__"], undefined, "(0)");
      sc["__proto__"] = { x: 1 };
      assert_eq(a.x, undefined, "(1)");
      assert_eq(sc.x, undefined, "(2)");
      assert_eq(sc["__proto__"], { x: 1 }, "(3)");
      assert_eq(Object.x, undefined, "(4)");
    } catch (e) {
      assert(e instanceof ScopeError, "error kind");
      assert_eq(e.message, "Tried modifying prototype");
    }
  });

  it("should work recursively", function () {
    let sc1 = mkScope(null);
    let sc2 = mkScope(sc1);
    sc1["a"] = 1;
    assert_eq(sc2["a"], 1, "(1)");
    sc2["a"] = 2;
    assert_eq(sc1["a"], 1, "(2)");
    assert_eq(sc2["a"], 2, "(3)");
    assert_eq(sc1[allKeys], ["a"], "(4)");
    assert_eq(sc2[allKeys], ["a"], "(5)");
    assert_eq(sc1[extractScope], { a: 1 }, "(6)");
    assert_eq(sc2[extractScope], { a: 2 }, "(7)");
  });
});

describe("mkScopeWith", function () {
  it("should deny modifications", function () {
    let sc = mkScopeWith();
    try {
      sc["a"] = 2;
      assert(false, "unreachable");
    } catch (e) {
      assert_eq(
        e.message,
        "Tried overwriting key 'a' in read-only scope",
        "error message"
      );
    }
  });

  it("should propagate get requests", function () {
    let scbase = mkScopeWith();
    let sc1 = mkScope(scbase);
    sc1["x"] = 1;
    let sc2 = mkScopeWith(sc1);
    try {
      sc2["x"] = 2;
      assert(false, "unreachable");
    } catch (e) {
      assert_eq(
        e.message,
        "Tried overwriting key 'x' in read-only scope",
        "error message"
      );
    }
    assert_eq(sc1[allKeys], ["x"], "(keys1)");
    assert_eq(sc2[allKeys], ["x"], "(keys2)");
    assert_eq(sc2["x"], 1, "(get)");
  });
});

describe("add", function () {
  it("should work if arguments are correct", async function () {
    assert_eq(await xblti.add(1200)(567), 1767, "integer");
    assert_eq(await xblti.add(-100)(567), 467, "integer (2)");
    assert_eq(await xblti.add(203)(-500), -297, "integer (3)");
  });
  describe("should report errors correctly", function () {
    it("string/string", async function () {
      try {
        console.log(await xblti.add("ab")("cde"));
        assert(false, "unreachable");
      } catch (e) {
        assert(e instanceof TypeError, "error kind");
        assert_eq(
          e.message,
          "builtins.add: invalid input type (string), expected (number)",
          "message"
        );
      }
    });
    it("int/string", async function () {
      try {
        console.log(await xblti.add(0)("oops"));
        assert(false, "unreachable");
      } catch (e) {
        assert(e instanceof TypeError, "error kind");
        assert_eq(
          e.message,
          "builtins.add: given types mismatch (number != string)",
          "message"
        );
      }
    });
    it("string/int", async function () {
      try {
        console.log(await xblti.add("oops")(0));
        assert(false, "unreachable");
      } catch (e) {
        assert(e instanceof TypeError, "error kind");
        assert_eq(
          e.message,
          "builtins.add: given types mismatch (string != number)",
          "message"
        );
      }
    });
  });
});

describe("compareVersions", function () {
  it("should work for simple cases", async function () {
    assert_eq(await xblti.compareVersions("1.0")("2.3"), -1, "(1)");
    assert_eq(await xblti.compareVersions("2.3")("1.0"), 1, "(2)");
    assert_eq(await xblti.compareVersions("2.1")("2.3"), -1, "(3)");
    assert_eq(await xblti.compareVersions("2.3")("2.3"), 0, "(4)");
    assert_eq(await xblti.compareVersions("2.5")("2.3"), 1, "(5)");
    assert_eq(await xblti.compareVersions("3.1")("2.3"), 1, "(6)");
  });
  it("should work for complex cases", async function () {
    assert_eq(await xblti.compareVersions("2.3.1")("2.3"), 1, "(7)");
    assert_eq(await xblti.compareVersions("2.3.1")("2.3a"), 1, "(8)");
    assert_eq(await xblti.compareVersions("2.3pre1")("2.3"), -1, "(9)");
    assert_eq(await xblti.compareVersions("2.3")("2.3pre1"), 1, "(10)");
    assert_eq(await xblti.compareVersions("2.3pre3")("2.3pre12"), -1, "(11)");
    assert_eq(await xblti.compareVersions("2.3pre12")("2.3pre3"), 1, "(12)");
    assert_eq(await xblti.compareVersions("2.3a")("2.3c"), -1, "(13)");
    assert_eq(await xblti.compareVersions("2.3c")("2.3a"), 1, "(14)");
    assert_eq(await xblti.compareVersions("2.3pre1")("2.3c"), -1, "(15)");
    assert_eq(await xblti.compareVersions("2.3pre1")("2.3q"), -1, "(16)");
    assert_eq(await xblti.compareVersions("2.3q")("2.3pre1"), 1, "(17)");
  });
});

describe("tryEval", function () {
  it("should work for PLazy.from", async function () {
    assert_eq(
      await xblti.tryEval(
        PLazy.from(async () => {
          throw new NixEvalError("boo");
        })
      ),
      { success: false, value: false }
    );
  });
  it("should work for async indirection", async function () {
    let x = (async () => {
      throw new NixEvalError("boo");
    })();
    assert_eq(await xblti.tryEval(x), { success: false, value: false });
  });
  it("should work for impure.nix/try<nixpkgs-overlays>", async function() {
    assert_eq(await PLazy.from(async () => {
      let nix__try = async (nix__x) => async (nix__def) =>
        PLazy.from(async () => {
          let nix__res = PLazy.from(
            async () => await xblti.tryEval(nix__x)
          );
          return await ((await (
            await nix__res
          ).success)
            ? (
                await nix__res
              ).value
            : nix__def);
        });
      return await (
        await (
          await nix__try
        )(xblti.toString((async () => { throw new NixEvalError('path-overlays.nix: export did not resolve: Store|nixpkgs-overlays'); })()))
      )("");
    }), "");
  });
});

describe("+", function () {
  it("should work if arguments are correct", async function () {
    assert_eq(await nixOp.Add(1200, 567), 1767, "integer");
    assert_eq(await nixOp.Add(-100, 567), 467, "integer (2)");
    assert_eq(await nixOp.Add(203, -500), -297, "integer (3)");
    assert_eq(await nixOp.Add("ab", "cde"), "abcde", "string");
  });
  describe("should report errors correctly", function () {
    it("int/string", async function () {
      try {
        console.log(await nixOp.Add(0, "oops"));
        assert(false, "unreachable");
      } catch (e) {
        assert(e instanceof TypeError, "error kind");
        assert_eq(
          e.message,
          "operator +: given types mismatch (number != string)",
          "message"
        );
      }
    });
    it("string/int", async function () {
      try {
        console.log(await nixOp.Add("oops", 0));
        assert(false, "unreachable");
      } catch (e) {
        assert(e instanceof TypeError, "error kind");
        assert_eq(
          e.message,
          "operator +: given types mismatch (string != number)",
          "message"
        );
      }
    });
  });
});

it("-", async function () {
  assert_eq(await nixOp.Sub(1200, 567), 633, "integer");
  assert_eq(await nixOp.Sub(-100, 567), -667, "integer (2)");
  assert_eq(await nixOp.Sub(203, -500), 703, "integer (3)");
});

it("*", async function () {
  assert_eq(await nixOp.Mul(50, 46), 2300, "integer");
  assert_eq(await nixOp.Mul(50004, 1023), 51154092, "integer (2)");
  assert_eq(await nixOp.Mul(203, -500), -101500, "integer (3)");
  assert_eq(await nixOp.Mul(-203, 500), -101500, "integer (4)");
  assert_eq(await nixOp.Mul(-203, -500), 101500, "integer (5)");
});

describe("/", function () {
  it("should work if arguments are correct", async function () {
    assert_eq(await nixOp.Div(1, 1), 1, "integer");
    assert_eq(await nixOp.Div(8, 4), 2, "integer (2)");
    assert_eq(await nixOp.Div(754677, 1331), 567, "integer (3)");
  });
  it("should catch division-by-zero", async function () {
    try {
      console.log(await nixOp.Div(1, 0));
      assert(false, "unreachable");
    } catch (e) {
      assert(e instanceof RangeError, "error kind");
      assert_eq(e.message, "Division by zero", "message");
    }
  });
});

describe("//", function () {
  it("should merge distinct attrsets correctly", async function () {
    assert_eq(await nixOp.Update({ a: 1 }, { b: 2 }), { a: 1, b: 2 });
  });
  it("should merge overlapping attrsets correctly", async function () {
    let a = { a: { i: 0 } };
    let b = { a: { i: 2 } };
    assert_eq(await nixOp.Update(a, b), { a: { i: 2 } }, "//");
    assert_eq(a, { a: { i: 0 } }, "original objects shouldn't be modified");
  });
});

it("==", async function () {
  assert_eq(await nixOp.Equal(1, 1), true);
});

it("!=", async function () {
  assert_eq(await nixOp.NotEqual(1, 1), false);
});
