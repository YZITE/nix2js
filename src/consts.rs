#[derive(Clone, Copy)]
pub enum IdentCateg {
    Literal(&'static str),

    // aliased builtin
    AlBuiltin(&'static str),

    // lambda argument
    LambdaArg,

    // used for simple let..in stmts
    LetLetVar,

    // also used for recursive attrsets
    LetInScopeVar,

    // rest
    WithScopeVar,
}

pub const NIX_BUILTINS_RT: &str = "nixBltiRT";
pub const NIX_OPERATORS: &str = "nixOp";
pub const NIX_EXTRACT_SCOPE: &str = "nixBlti.extractScope";
pub const NIX_OR_DEFAULT: &str = "nixBlti.orDefault";
pub const NIX_RUNTIME: &str = "nixRt";
pub const NIX_IN_SCOPE: &str = "nixInScope";
pub const NIX_LAMBDA_ARG_PFX: &str = "nix__";
pub const NIX_LAMBDA_BOUND: &str = "nixBound";

use IdentCateg::*;
pub const DFL_VARS: &[(&str, IdentCateg)] = &[
    ("abort", AlBuiltin("abort")),
    ("__addErrorContext", AlBuiltin("__addErrorContext")),
    ("__add", AlBuiltin("__add")),
    ("__all", AlBuiltin("__all")),
    ("__any", AlBuiltin("__any")),
    ("__appendContext", AlBuiltin("__appendContext")),
    ("__attrNames", AlBuiltin("__attrNames")),
    ("__attrValues", AlBuiltin("__attrValues")),
    ("baseNameOf", AlBuiltin("baseNameOf")),
    ("__bitAnd", AlBuiltin("__bitAnd")),
    ("__bitOr", AlBuiltin("__bitOr")),
    ("__bitXor", AlBuiltin("__bitXor")),
    ("builtins", Literal(NIX_BUILTINS_RT)),
    ("__catAttrs", AlBuiltin("__catAttrs")),
    ("__compareVersions", AlBuiltin("__compareVersions")),
    ("__concatLists", AlBuiltin("__concatLists")),
    ("__concatMap", AlBuiltin("__concatMap")),
    ("__concatStringsSep", AlBuiltin("__concatStringsSep")),
    ("__currentSystem", AlBuiltin("__currentSystem")),
    ("__currentTime", AlBuiltin("__currentTime")),
    ("__deepSeq", AlBuiltin("__deepSeq")),
    ("derivation", AlBuiltin("derivation")),
    ("derivationStrict", AlBuiltin("derivationStrict")),
    ("dirOf", AlBuiltin("dirOf")),
    ("__div", AlBuiltin("__div")),
    ("__elemAt", AlBuiltin("__elemAt")),
    ("__elem", AlBuiltin("__elem")),
    ("false", Literal("false")),
    ("fetchGit", AlBuiltin("fetchGit")),
    ("fetchMercurial", AlBuiltin("fetchMercurial")),
    ("fetchTarball", AlBuiltin("fetchTarball")),
    ("__fetchurl", AlBuiltin("__fetchurl")),
    ("__filter", AlBuiltin("__filter")),
    ("__filterSource", AlBuiltin("__filterSource")),
    ("__findFile", AlBuiltin("__findFile")),
    ("__foldl'", AlBuiltin("__foldl'")),
    ("__fromJSON", AlBuiltin("__fromJSON")),
    ("fromTOML", AlBuiltin("fromTOML")),
    ("__functionArgs", AlBuiltin("__functionArgs")),
    ("__genericClosure", AlBuiltin("__genericClosure")),
    ("__genList", AlBuiltin("__genList")),
    ("__getAttr", AlBuiltin("__getAttr")),
    ("__getContext", AlBuiltin("__getContext")),
    ("__getEnv", AlBuiltin("__getEnv")),
    ("__hasAttr", AlBuiltin("__hasAttr")),
    ("__hasContext", AlBuiltin("__hasContext")),
    ("__hashFile", AlBuiltin("__hashFile")),
    ("__hashString", AlBuiltin("__hashString")),
    ("__head", AlBuiltin("__head")),
    ("import", AlBuiltin("import")),
    ("__intersectAttrs", AlBuiltin("__intersectAttrs")),
    ("__isAttrs", AlBuiltin("__isAttrs")),
    ("__isBool", AlBuiltin("__isBool")),
    ("__isFloat", AlBuiltin("__isFloat")),
    ("__isFunction", AlBuiltin("__isFunction")),
    ("__isInt", AlBuiltin("__isInt")),
    ("__isList", AlBuiltin("__isList")),
    ("isNull", AlBuiltin("isNull")),
    ("__isPath", AlBuiltin("__isPath")),
    ("__isString", AlBuiltin("__isString")),
    ("__langVersion", AlBuiltin("__langVersion")),
    ("__length", AlBuiltin("__length")),
    ("__lessThan", AlBuiltin("__lessThan")),
    ("__listToAttrs", AlBuiltin("__listToAttrs")),
    ("__mapAttrs", AlBuiltin("__mapAttrs")),
    ("map", AlBuiltin("map")),
    ("__match", AlBuiltin("__match")),
    ("__mul", AlBuiltin("__mul")),
    ("__nixPath", AlBuiltin("__nixPath")),
    ("__nixVersion", AlBuiltin("__nixVersion")),
    ("null", Literal("null")),
    ("__parseDrvName", AlBuiltin("__parseDrvName")),
    ("__partition", AlBuiltin("__partition")),
    ("__pathExists", AlBuiltin("__pathExists")),
    ("__path", AlBuiltin("__path")),
    ("placeholder", AlBuiltin("placeholder")),
    ("__readDir", AlBuiltin("__readDir")),
    ("__readFile", AlBuiltin("__readFile")),
    ("removeAttrs", AlBuiltin("removeAttrs")),
    ("__replaceStrings", AlBuiltin("__replaceStrings")),
    ("scopedImport", AlBuiltin("scopedImport")),
    ("__seq", AlBuiltin("__seq")),
    ("__sort", AlBuiltin("__sort")),
    ("__split", AlBuiltin("__split")),
    ("__splitVersion", AlBuiltin("__splitVersion")),
    ("__storeDir", AlBuiltin("__storeDir")),
    ("__storePath", AlBuiltin("__storePath")),
    ("__stringLength", AlBuiltin("__stringLength")),
    ("__sub", AlBuiltin("__sub")),
    ("__substring", AlBuiltin("__substring")),
    ("__tail", AlBuiltin("__tail")),
    ("throw", AlBuiltin("throw")),
    ("__toFile", AlBuiltin("__toFile")),
    ("__toJSON", AlBuiltin("__toJSON")),
    ("__toPath", AlBuiltin("__toPath")),
    ("toString", AlBuiltin("toString")),
    ("__toXML", AlBuiltin("__toXML")),
    ("__trace", AlBuiltin("__trace")),
    ("true", Literal("true")),
    ("__tryEval", AlBuiltin("__tryEval")),
    ("__typeOf", AlBuiltin("__typeOf")),
    (
        "__unsafeDiscardOutputDependency",
        AlBuiltin("__unsafeDiscardOutputDependency"),
    ),
    (
        "__unsafeDiscardStringContext",
        AlBuiltin("__unsafeDiscardStringContext"),
    ),
    ("__unsafeGetAttrPos", AlBuiltin("__unsafeGetAttrPos")),
    ("__valueSize", AlBuiltin("__valueSize")),
];
