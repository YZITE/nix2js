/**
 this crate converts Nix code into javascript code, and generates code
 which is wrapped inside a function which expects a "runtime" function,
 which is called with the invoking line number and should return an object,
 which should contain the following methods:
 - `throw(message)`: throws a javascript exception,
    should automatically supply the correct file name
    (triggered by `assert` and `throw` if hit)
 - `abort(message)`: like `error`, but triggered by `abort` if hit
 - `derive(derivation_attrset)`: should realise a derivation
    (used e.g. for import-from-derivation, also gets linked onto derivations)
 - `export(anchor,path)`: export a path into the nix store
 - `import(to_be_imported_path)`: import a nix file,
    should callback into the parser.

 It also expects a `nixBlti` object as the second argument, which should
 be the objects/namespace of all exported objects of the npm package `nix-builtins`.
**/
use rnix::{types::*, SyntaxNode as NixNode};

const NIX_BUILTINS_RT: &str = "nixBltiRT";
const NIX_LAZY: &str = "nixBlti.Lazy";
const NIX_MKLAZY: &str = "nixBlti.mkLazy";
const NIX_DELAY: &str = "nixBlti.delay";
const NIX_FORCE: &str = "nixBlti.force";
const NIX_OR_DEFAULT: &str = "nixBlti.orDefault";
const NIX_RUNTIME: &str = "nixRt";
const NIX_IN_SCOPE: &str = "nixInScope";
const NIX_MK_SCOPE: &str = "nixBlti.mkScope(nixInScope)";
const NIX_LAMBDA_ARG_PFX: &str = "nix__";
const NIX_LAMBDA_BOUND: &str = "nixBound";

enum ScopedVar {
    LambdaArg,
}

struct Context<'a> {
    inp: &'a str,
    acc: &'a mut String,
    vars: Vec<(String, ScopedVar)>,
}

fn escape_str(s: &str) -> String {
    serde_json::value::Value::String(s.to_string()).to_string()
}

macro_rules! err {
    ($x:expr) => {{
        return Err(vec![$x]);
    }};
}

impl Context<'_> {
    fn push(&mut self, x: &str) {
        *self.acc += x;
    }

    fn txtrng_to_lineno(&self, txtrng: rnix::TextRange) -> usize {
        let bytepos: usize = txtrng.start().into();
        self.inp
            .char_indices()
            .take_while(|(idx, _)| *idx <= bytepos)
            .filter(|(_, c)| *c == '\n')
            .count()
    }

    fn rtv(
        &mut self,
        txtrng: rnix::TextRange,
        x: Option<NixNode>,
        desc: &str,
    ) -> Result<(), Vec<String>> {
        match x {
            None => {
                err!(format!(
                    "line {}: {} missing",
                    self.txtrng_to_lineno(txtrng),
                    desc
                ));
            }
            Some(x) => self.translate_node(x),
        }
    }

    fn translate_varname(&self, vn: &str, txtrng: rnix::TextRange) -> String {
        use ScopedVar as Sv;
        match vn {
            "builtins" => {
                // keep the builtins informed about the line number
                format!("{}({})", NIX_BUILTINS_RT, self.txtrng_to_lineno(txtrng))
            }
            "derivation" => {
                // aliased name for derivation builtin
                format!(
                    "{}({}).derivation",
                    NIX_BUILTINS_RT,
                    self.txtrng_to_lineno(txtrng),
                )
            }
            "abort" | "import" | "throw" => {
                format!("{}({}).{}", NIX_RUNTIME, self.txtrng_to_lineno(txtrng), vn,)
            }
            _ => match self.vars.iter().rev().find(|(ref i, _)| vn == i) {
                Some((_, x)) => match x {
                    Sv::LambdaArg => format!("{}{}", NIX_LAMBDA_ARG_PFX, vn),
                },
                None => format!("{}({})", NIX_IN_SCOPE, escape_str(vn)),
            },
        }
    }

    fn translate_ident(&self, id: &Ident) -> String {
        self.translate_varname(id.as_str(), id.node().text_range())
    }

    fn translate_node_kv(&mut self, i: KeyValue, scope: &str) -> Result<(), Vec<String>> {
        let txtrng = i.node().text_range();
        let kp: Vec<_> = if let Some(key) = i.key() {
            key.path().collect()
        } else {
            err!(format!(
                "line {}: key for key-value pair missing",
                self.txtrng_to_lineno(txtrng)
            ));
        };

        if let [name] = &kp[..] {
            if let Some(name) = Ident::cast(name.clone()) {
                self.push(&format!(
                    "{}({},{}(()=>(",
                    scope,
                    escape_str(name.as_str()),
                    NIX_MKLAZY
                ));
                self.rtv(txtrng, i.value(), "value for key-value pair")?;
                self.push(")));");
            } else {
                unimplemented!("unsupported key-value pair: {:?}", kp);
            }
        } else {
            unimplemented!("unsupported key-value pair: {:?}", kp);
        }
        Ok(())
    }

    fn translate_let<EH: EntryHolder>(
        &mut self,
        node: &EH,
        body: NixNode,
    ) -> Result<(), Vec<String>> {
        self.push(&format!("(({})=>{{", NIX_IN_SCOPE));
        for i in node.entries() {
            self.translate_node_kv(i, NIX_IN_SCOPE)?;
        }
        for i in node.inherits() {
            self.translate_node(i.node().clone())?;
        }
        self.push("return ");
        self.translate_node(body)?;
        self.push(&format!(";}})({})", NIX_MK_SCOPE));
        Ok(())
    }

    fn translate_node(&mut self, node: NixNode) -> Result<(), Vec<String>> {
        if node.kind().is_trivia() {
            return Ok(());
        }

        let txtrng = node.text_range();
        let x = match ParsedType::try_from(node) {
            Err(e) => {
                err!(format!(
                    "{:?} (line {}): unable to parse node of kind {:?}",
                    txtrng,
                    self.txtrng_to_lineno(txtrng),
                    e.0
                ));
            }
            Ok(x) => x,
        };
        use ParsedType as Pt;
        let builtins = self.translate_varname("builtins", txtrng);

        match x {
            Pt::Apply(app) => {
                self.push("(");
                self.rtv(txtrng, app.lambda(), "lambda for application")?;
                self.push(")(");
                self.rtv(txtrng, app.value(), "value for application")?;
                self.push(")");
            }

            Pt::Assert(art) => {
                self.push("(()=>{");
                self.push(&builtins);
                self.push(".assert(");
                self.rtv(txtrng, art.condition(), "condition for assert")?;
                self.push("); return (");
                self.rtv(txtrng, art.body(), "body for assert")?;
                self.push("); })()");
            }

            Pt::AttrSet(_) => unimplemented!(),

            Pt::BinOp(bo) => {
                if let Some(op) = bo.operator() {
                    use BinOpKind as Bok;
                    match op {
                        Bok::IsSet => {
                            self.push(&format!("new {lazy}(()=>(", lazy = NIX_LAZY));
                            self.rtv(txtrng, bo.lhs(), "lhs for binop ?")?;
                            self.push(").hasOwnProperty(");
                            if let Some(x) = bo.rhs() {
                                if let Some(y) = Ident::cast(x.clone()) {
                                    self.push(&escape_str(y.as_str()));
                                } else {
                                    self.push(&format!("{force}(()=>", force = NIX_FORCE));
                                    self.translate_node(x)?;
                                    self.push(")");
                                }
                            } else {
                                err!(format!(
                                    "line {}: rhs for binop ? missing",
                                    self.txtrng_to_lineno(txtrng),
                                ));
                            }
                            self.push("))");
                        }
                        _ => {
                            self.push(&format!("{}.nixop__{:?}", builtins, op));
                            self.push(&format!("({mklazy}(()=>", mklazy = NIX_MKLAZY));
                            self.rtv(txtrng, bo.lhs(), "lhs for binop")?;
                            self.push(&format!("),{mklazy}(()=>", mklazy = NIX_MKLAZY));
                            self.rtv(txtrng, bo.rhs(), "lhs for binop")?;
                            self.push("))");
                        }
                    }
                } else {
                    err!(format!(
                        "line {}: operator for binop missing",
                        self.txtrng_to_lineno(txtrng),
                    ));
                }
            }

            Pt::Dynamic(d) => {
                // dynamic key component
                self.push(NIX_FORCE);
                self.push("(");
                self.rtv(txtrng, d.inner(), "inner for dynamic (key)")?;
                self.push(")");
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => self.push(&self.translate_ident(&id)),

            Pt::IfElse(ie) => {
                self.push("new ");
                self.push(NIX_LAZY);
                self.push("(function(){let nixRet=undefined;if(");
                self.push(NIX_FORCE);
                self.push("(");
                self.rtv(txtrng, ie.condition(), "condition for if-else")?;
                self.push(")){nixRet=");
                self.rtv(txtrng, ie.body(), "if-body for if-else")?;
                self.push(";}else{nixRet=");
                self.rtv(txtrng, ie.else_body(), "else-body for if-else")?;
                self.push(";}return nixRet;})");
            }

            Pt::Inherit(inh) => {
                // TODO: the following stuff belongs in the handling of
                // rec attrsets and let bindings
                //self.push("((function(){");
                //self.push("let nixInScope = inScope(nixInScope, undefined);");
                // idk how to handle self-references....
                //unimplemented!();
                //self.push("})())");

                if let Some(inhf) = inh.from() {
                    self.push("(function(){let nixInhR=");
                    self.rtv(txtrng, inhf.inner(), "inner for inherit-from")?;
                    self.push(";");
                    for id in inh.idents() {
                        let idesc = escape_str(id.as_str());
                        self.push(NIX_IN_SCOPE);
                        self.push("(");
                        self.push(&idesc);
                        self.push(",new ");
                        self.push(NIX_LAZY);
                        self.push("(()=>nixInhR[");
                        self.push(&idesc);
                        self.push("];));");
                    }
                    self.push("})()");
                } else {
                    for id in inh.idents() {
                        let idas = id.as_str();
                        self.push(NIX_IN_SCOPE);
                        self.push("(");
                        self.push(&escape_str(idas));
                        self.push(",");
                        self.push(&self.translate_ident(&id));
                        self.push(");");
                    }
                }
            }

            Pt::InheritFrom(inhf) => self.rtv(txtrng, inhf.inner(), "inner for inherit-from")?,

            Pt::Key(key) => {
                let mut fi = true;
                self.push("[");
                for i in key.path() {
                    if fi {
                        fi = false;
                    } else {
                        self.push(",");
                    }
                    self.translate_node(i)?;
                }
                self.push("]");
            }

            Pt::KeyValue(kv) => unreachable!("standalone key-value not supported: {:?}", kv),

            Pt::Lambda(lam) => {
                if let Some(x) = lam.arg() {
                    // FIXME: use guard to truncate vars
                    let cur_lamstk = self.vars.len();
                    self.push("(function(");
                    if let Some(y) = Ident::cast(x.clone()) {
                        let yas = y.as_str();
                        self.vars.push((yas.to_string(), ScopedVar::LambdaArg));
                        self.push(&self.translate_ident(&y));
                        self.push("){");
                        // } -- this fixes brace association
                    } else if let Some(y) = Pattern::cast(x) {
                        let argname = if let Some(z) = y.at() {
                            self.vars
                                .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                            self.translate_ident(&z)
                        } else {
                            NIX_LAMBDA_BOUND.to_string()
                        };
                        self.push(&format!(
                            "{arg}){{let {arg}={}({arg});",
                            NIX_FORCE,
                            arg = argname
                        ));
                        // } -- this fixes brace association
                        for i in y.entries() {
                            if let Some(z) = i.name() {
                                self.vars
                                    .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                                if let Some(zdfl) = i.default() {
                                    self.push(&format!(
                                        "let {zname}=({arg}.{zas} !== undefined)?({arg}.{zas}):(",
                                        arg = argname,
                                        zas = z.as_str(),
                                        zname = self.translate_ident(&z)
                                    ));
                                    self.translate_node(zdfl)?;
                                    self.push(");");
                                } else {
                                    // TODO: adjust error message to what Nix currently issues.
                                    self.push(&format!(
                                        "let {zname}={arg}.{zas};if({zname}===undefined){{{rt}({lno}).error(\"attrset element {zas} missing at lambda call\");}} ",
                                        arg = argname,
                                        zas = z.as_str(),
                                        zname = self.translate_ident(&z),
                                        rt = NIX_RUNTIME,
                                        lno = self.txtrng_to_lineno(z.node().text_range())
                                    ));
                                }
                            } else {
                                err!(format!("lambda pattern ({:?}) has entry without name", y));
                            }
                        }
                    } else {
                        err!(format!("lambda ({:?}) with invalid argument", lam));
                    }

                    self.rtv(txtrng, lam.body(), "body for lambda")?;
                    assert!(self.vars.len() >= cur_lamstk);
                    self.vars.truncate(cur_lamstk);
                    self.push("})");
                } else {
                    err!(format!("lambda ({:?}) with missing argument", lam));
                }
            }

            Pt::LegacyLet(l) => self.translate_let(
                &l,
                l.entries()
                    .find(|i| {
                        let kp: Vec<_> = if let Some(key) = i.key() {
                            key.path().collect()
                        } else {
                            return false;
                        };
                        if let [name] = &kp[..] {
                            if let Some(name) = Ident::cast(name.clone()) {
                                if name.as_str() == "body" {
                                    return true;
                                }
                            }
                        }
                        false
                    })
                    .and_then(|i| i.value())
                    .ok_or_else(|| {
                        vec![format!(
                            "line {}: legacy let {{ ... }} without body assignment",
                            self.txtrng_to_lineno(l.node().text_range())
                        )]
                    })?,
            )?,

            Pt::LetIn(l) => self.translate_let(
                &l,
                l.body().ok_or_else(|| {
                    vec![format!(
                        "line {}: let ... in ... without body",
                        self.txtrng_to_lineno(l.node().text_range())
                    )]
                })?,
            )?,

            Pt::List(l) => {
                self.push("[");
                let mut fi = true;
                for i in l.items() {
                    if fi {
                        fi = false;
                    } else {
                        self.push(",");
                    }
                    self.translate_node(i)?;
                }
                self.push("]");
            }

            Pt::OrDefault(od) => {
                self.push(&format!(
                    "{ordfl}({mklazy}(()=>(",
                    ordfl = NIX_OR_DEFAULT,
                    mklazy = NIX_MKLAZY,
                ));
                self.rtv(
                    txtrng,
                    od.index().map(|i| i.node().clone()),
                    "or-default without indexing operation",
                )?;
                self.push(&format!(")),{delay}(", delay = NIX_DELAY));
                self.rtv(txtrng, od.default(), "or-default without default")?;
                self.push("))");
            }

            Pt::Paren(p) => self.rtv(txtrng, p.inner(), "inner for paren")?,
            Pt::PathWithInterpol(p) => {
                unreachable!("standalone path-with-interpolation not supported: {:?}", p)
            }
            Pt::Pattern(p) => unreachable!("standalone pattern not supported: {:?}", p),
            Pt::PatBind(p) => unreachable!("standalone pattern @ bind not supported: {:?}", p),
            Pt::PatEntry(p) => unreachable!("standalone pattern entry not supported: {:?}", p),

            Pt::Root(r) => self.rtv(txtrng, r.inner(), "inner for root")?,

            Pt::Select(sel) => {
                self.push("(");
                self.rtv(txtrng, sel.set(), "set for select")?;
                self.push(")[");
                if let Some(idx) = sel.index() {
                    if let Some(val) = Ident::cast(idx.clone()) {
                        self.push(&escape_str(val.as_str()));
                    } else {
                        self.translate_node(idx)?;
                    }
                } else {
                    err!(format!("{:?}: {} missing", txtrng, "index for selectr"));
                }
                self.push("]");
            }

            Pt::Str(s) => {
                self.push("(");
                let mut fi = true;
                for i in s.parts() {
                    if fi {
                        fi = false;
                    } else {
                        self.push("+");
                    }
                    use rnix::value::StrPart as Sp;
                    match i {
                        Sp::Literal(lit) => self.push(&escape_str(&lit)),
                        Sp::Ast(ast) => {
                            self.push("(");
                            let txtrng = ast.node().text_range();
                            self.rtv(txtrng, ast.inner(), "inner for str-interpolate")?;
                            self.push(")");
                        }
                    }
                }
                self.push(")");
            }

            Pt::StrInterpol(si) => self.rtv(txtrng, si.inner(), "inner for str-interpolate")?,

            Pt::UnaryOp(uo) => {
                use UnaryOpKind as Uok;
                match uo.operator() {
                    Uok::Invert | Uok::Negate => {}
                }
                self.push(&format!("{}.nixuop__{:?}(", builtins, uo.operator()));
                self.rtv(txtrng, uo.value(), "value for unary-op")?;
                self.push(")");
            }

            Pt::Value(v) => match v.to_value() {
                Ok(x) => {
                    use rnix::value::Value as NixVal;
                    use serde_json::value::{Number as JsNum, Value as JsVal};
                    let jsvs = match x {
                        NixVal::Float(flt) => {
                            JsVal::Number(JsNum::from_f64(flt).expect("unrepr-able float"))
                                .to_string()
                        }
                        NixVal::Integer(int) => JsVal::Number(int.into()).to_string(),
                        NixVal::String(s) => JsVal::String(s).to_string(),
                        NixVal::Path(anch, path) => {
                            format!(
                                "{}({}).export({},{})",
                                NIX_RUNTIME,
                                self.txtrng_to_lineno(txtrng),
                                escape_str(&format!("{:?}", anch)),
                                escape_str(&path),
                            )
                        }
                    };
                    self.push(&jsvs);
                }
                Err(e) => err!(format!(
                    "line {}: value deserialization error: {}",
                    self.txtrng_to_lineno(txtrng),
                    e
                )),
            },

            Pt::With(_) => unimplemented!(),
        }

        Ok(())
    }
}

pub fn translate(s: &str) -> Result<String, Vec<String>> {
    let parsed = rnix::parse(s);

    // return any occured parsing errors
    {
        let errs = parsed.errors();
        if !errs.is_empty() {
            return Err(errs.into_iter().map(|i| i.to_string()).collect());
        }
    }

    let mut ret = String::new();
    ret += "(function(nixRt,nixBlti){";
    ret += NIX_BUILTINS_RT;
    ret += "=nixBlti.initRtDep(nixRt);let ";
    ret += NIX_IN_SCOPE;
    ret += "=undefined;return ";
    Context {
        inp: s,
        acc: &mut ret,
        vars: Default::default(),
    }
    .translate_node(parsed.node())?;
    ret += ";})";
    Ok(ret)
}
