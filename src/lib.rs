/**
 this crate converts Nix code into javascript code, and generates code
 which is wrapped inside a function which expects a "runtime" object,
 which should contain the following methods:
 - `error(message, lineno)`: throws a javascript exception,
    should automatically supply the correct file name
    (triggered by `assert` and `throw` if hit)
 - `derive(derivation_attrset)`: should realise a derivation
    (used e.g. for import-from-derivation, also gets linked onto derivations)
 - `import(cwd, to_be_imported_path)`: import a nix file,
    should callback into the parser.

 It also expects a `nixBlti` object as the second argument, which should
 be the objects/namespace of all exported objects of the npm package `nix-builtins`.
**/
use rnix::{types::*, SyntaxNode as NixNode};

const NIX_BUILTINS_RT: &str = "nixBltiRT";
const NIX_LAZY: &str = "nixBlti.Lazy";
const NIX_DELAY: &str = "nixBlti.delay";
const NIX_FORCE: &str = "nixBlti.force";
const NIX_OR_DEFAULT: &str = "nixBlti.orDefault";
const NIXBLT_IN_SCOPE: &str = "nixBlti.inScope";
const NIX_RUNTIME: &str = "nixRt";
const NIX_IN_SCOPE: &str = "nixInScope";
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
    s.replace("\\", "\\\\").replace("\"", "\\\"")
}

impl Context<'_> {
    fn txtrng_to_lineno(&self, txtrng: rnix::TextRange) -> usize {
        let bytepos: usize = txtrng.start().into();
        self.inp
            .char_indices()
            .take_while(|(idx, _)| *idx <= bytepos)
            .filter(|(_, c)| *c == '\n')
            .count()
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
            _ => match self.vars.iter().rev().find(|(ref i, _)| vn == i) {
                Some((_, x)) => match x {
                    Sv::LambdaArg => format!("{}{}", NIX_LAMBDA_ARG_PFX, vn),
                },
                None => format!("{}(\"{}\")", NIX_IN_SCOPE, escape_str(vn)),
            },
        }
    }

    fn translate_ident(&self, id: &Ident) -> String {
        self.translate_varname(id.as_str(), id.node().text_range())
    }

    fn use_or(&mut self, x: Option<NixNode>, alt: &str) -> Result<(), Vec<String>> {
        match x {
            None => {
                *self.acc += alt;
                Ok(())
            }
            Some(x) => self.translate_node(x),
        }
    }

    fn translate_let<EH: EntryHolder>(
        &mut self,
        node: &EH,
        body: NixNode,
    ) -> Result<(), Vec<String>> {
        unimplemented!()
    }

    fn translate_node(&mut self, node: NixNode) -> Result<(), Vec<String>> {
        if node.kind().is_trivia() {
            return Ok(());
        }

        macro_rules! err {
            ($x:expr) => {{
                return Err(vec![$x]);
            }};
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

        macro_rules! apush {
            ($x:expr) => {{
                *self.acc += $x;
            }};
        }
        macro_rules! rtv {
            ($x:expr, $desc:expr) => {{
                match $x {
                    None => {
                        err!(format!(
                            "line {}: {} missing",
                            self.txtrng_to_lineno(txtrng),
                            $desc
                        ));
                    }
                    Some(x) => self.translate_node(x)?,
                }
            }};
        }

        match x {
            Pt::Apply(app) => {
                apush!("(");
                rtv!(app.lambda(), "lambda for application");
                apush!(")(");
                rtv!(app.value(), "value for application");
                apush!(")");
            }

            Pt::Assert(art) => {
                apush!("(function() { ");
                apush!(NIX_BUILTINS_RT);
                apush!(".assert(");
                rtv!(art.condition(), "condition for assert");
                apush!("); return (");
                rtv!(art.body(), "body for assert");
                apush!("); })()");
            }

            Pt::BinOp(bo) => {
                // we need extra parens to get the operator precedence right
                apush!("((");
                rtv!(bo.lhs(), "lhs for binop");
                apush!(")");
                if let Some(op) = bo.operator() {
                    unimplemented!();
                } else {
                    err!(format!(
                        "line {}: operator for binop missing",
                        self.txtrng_to_lineno(txtrng),
                    ));
                }
                apush!("(");
                rtv!(bo.rhs(), "lhs for binop");
                apush!("))");
            }

            Pt::Dynamic(d) => {
                // dynamic key component
                apush!(NIX_FORCE);
                apush!("(");
                rtv!(d.inner(), "inner for dynamic (key)");
                apush!(")");
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => apush!(&self.translate_ident(&id)),

            Pt::IfElse(ie) => {
                apush!("new ");
                apush!(NIX_LAZY);
                apush!("(function() { let nixRet = undefined; if(");
                apush!(NIX_FORCE);
                apush!("(");
                rtv!(ie.condition(), "condition for if-else");
                apush!(")) { nixRet = ");
                rtv!(ie.body(), "if-body for if-else");
                apush!("; } else { nixRet = ");
                rtv!(ie.else_body(), "else-body for if-else");
                apush!("; }})");
            }

            Pt::Inherit(inh) => {
                // TODO: the following stuff belongs in the handling of
                // rec attrsets and let bindings
                //apush!("((function(){");
                //apush!("let nixInScope = inScope(nixInScope, undefined);");
                // idk how to handle self-references....
                //unimplemented!();
                //apush!("})())");

                if let Some(inhf) = inh.from() {
                    apush!("(function(){ let nixInhR = ");
                    rtv!(inhf.inner(), "inner for inherit-from");
                    apush!(";");
                    for id in inh.idents() {
                        let idesc = escape_str(id.as_str());
                        apush!(NIX_IN_SCOPE);
                        apush!("(\"");
                        apush!(&idesc);
                        apush!("\",new ");
                        apush!(NIX_LAZY);
                        apush!("(()=>nixInhR[\"");
                        apush!(&idesc);
                        apush!("\"];));");
                    }
                    apush!("})()");
                } else {
                    for id in inh.idents() {
                        let idas = id.as_str();
                        apush!(NIX_IN_SCOPE);
                        apush!("(\"");
                        apush!(&escape_str(idas));
                        apush!("\",");
                        apush!(&self.translate_ident(&id));
                        apush!(");");
                    }
                }
            }

            Pt::InheritFrom(inhf) => rtv!(inhf.inner(), "inner for inherit-from"),

            Pt::Key(key) => {
                let mut fi = true;
                apush!("[");
                for i in key.path() {
                    if fi {
                        fi = false;
                    } else {
                        apush!(",");
                    }
                    self.translate_node(i)?;
                }
                apush!("]");
            }

            Pt::Lambda(lam) => {
                if let Some(x) = lam.arg() {
                    // FIXME: use guard to truncate vars
                    let cur_lamstk = self.vars.len();
                    apush!("(function(");
                    if let Some(y) = Ident::cast(x.clone()) {
                        let yas = y.as_str();
                        self.vars.push((yas.to_string(), ScopedVar::LambdaArg));
                        apush!(&self.translate_ident(&y));
                        apush!("){");
                        // } -- this fixes brace association
                    } else if let Some(y) = Pattern::cast(x) {
                        let argname = if let Some(z) = y.at() {
                            self.vars
                                .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                            self.translate_ident(&z)
                        } else {
                            NIX_LAMBDA_BOUND.to_string()
                        };
                        apush!(&format!(
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
                                    apush!(&format!(
                                        "let {zname}=({arg}.{zas} !== undefined)?({arg}.{zas}):(",
                                        arg = argname,
                                        zas = z.as_str(),
                                        zname = self.translate_ident(&z)
                                    ));
                                    self.translate_node(zdfl);
                                    apush!(");");
                                } else {
                                    // TODO: adjust error message to what Nix currently issues.
                                    apush!(&format!(
                                        "let {zname}={arg}.{zas};if({zname}===undefined){{{rt}.error(\"attrset element {zas} missing at lambda call\",{lno});}} ",
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

                    rtv!(lam.body(), "body for lambda");
                    assert!(self.vars.len() >= cur_lamstk);
                    self.vars.truncate(cur_lamstk);
                    apush!("})");
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
                apush!("[");
                let mut fi = true;
                for i in l.items() {
                    if fi {
                        fi = false;
                    } else {
                        apush!(",");
                    }
                    self.translate_node(i)?;
                }
                apush!("]");
            }

            Pt::OrDefault(od) => {
                apush!(NIX_OR_DEFAULT);
                apush!("(new ");
                apush!(NIX_LAZY);
                apush!("(()=>");
                apush!(NIX_FORCE);
                apush!("(");
                rtv!(
                    od.index().map(|i| i.node().clone()),
                    "or-default without indexing operation"
                );
                apush!(")),");
                apush!(NIX_DELAY);
                apush!("(");
                rtv!(od.default(), "or-default without default");
                apush!("))");
            }

            Pt::Paren(p) => rtv!(p.inner(), "inner for paren"),
            Pt::Root(r) => rtv!(r.inner(), "inner for root"),

            Pt::Select(sel) => {
                apush!("(");
                rtv!(sel.set(), "set for select");
                apush!(")[");
                if let Some(idx) = sel.index() {
                    if let Some(val) = Ident::cast(idx.clone()) {
                        apush!("\"");
                        apush!(val.as_str());
                        apush!("\"");
                    } else {
                        self.translate_node(idx)?;
                    }
                } else {
                    err!(format!("{:?}: {} missing", txtrng, "index for selectr"));
                }
                apush!("]");
            }

            _ => unimplemented!(),
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
    ret += "(function(nixRt,nixBlti) { ";
    ret += NIX_BUILTINS_RT;
    ret += " = nixBlti.initRtDep(nixRt); let ";
    ret += NIX_IN_SCOPE;
    ret += " = function(key, value) { console.error(\"illegal nixInScope call with key=\", key, \" value=\", value); };\n";
    Context {
        inp: s,
        acc: &mut ret,
        vars: Default::default(),
    }
    .translate_node(parsed.node())?;
    ret += "\n})";
    Ok(ret)
}
