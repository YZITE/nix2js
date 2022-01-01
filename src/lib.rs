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
use std::collections::HashMap;

const NIX_BUILTINS_RT: &str = "nixBltiRT";
const NIX_LAZY: &str = "nixBlti.Lazy";
const NIX_FORCE: &str = "nixBlti.force";
const NIXBLT_IN_SCOPE: &str = "nixBlti.inScope";
const NIX_RUNTIME: &str = "nixRt";
const NIX_IN_SCOPE: &str = "nixInScope";
const NIX_LAMBDA_ARG_PFX: &str = "nix__";

enum ScopedVar {
    LambdaArg,
}

struct Context<'a> {
    acc: &'a mut String,
    vars: HashMap<String, ScopedVar>,
}

fn escape_str(s: &str) -> String {
    s.replace("\\", "\\\\").replace("\"", "\\\"")
}

impl Context<'_> {
    fn translate_varname(&self, vn: &str) -> String {
        use ScopedVar as Sv;
        match self.vars.get(vn) {
            Some(x) => match x {
                Sv::LambdaArg => format!("{}{}", NIX_LAMBDA_ARG_PFX, vn),
            },
            None => format!("({}(\"{}\"))", NIX_IN_SCOPE, escape_str(vn)),
        }
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

    fn translate_node(&mut self, node: NixNode) -> Result<(), Vec<String>> {
        if node.kind().is_trivia() {
            return Ok(());
        }

        let txtrng = node.text_range();
        let x = match ParsedType::try_from(node) {
            Err(e) => {
                return Err(vec![format!(
                    "{:?}: unable to parse node of kind {:?}",
                    txtrng, e.0
                )])
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
                    None => return Err(vec![format!("{:?}: {} missing", txtrng, $desc)]),
                    Some(x) => self.translate_node(x)?,
                }
            }};
        }

        match x {
            Pt::Apply(app) => {
                apush!("((");
                rtv!(app.lambda(), "lambda for application");
                apush!(")(");
                rtv!(app.value(), "value for application");
                apush!("))");
            }

            Pt::Assert(art) => {
                apush!("((function() { ");
                apush!(NIX_BUILTINS_RT);
                apush!(".assert(");
                rtv!(art.condition(), "condition for assert");
                apush!("); return (");
                rtv!(art.body(), "body for assert");
                apush!("); })())");
            }

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

            Pt::Dynamic(d) => {
                // dynamic key component
                apush!(NIX_FORCE);
                apush!("(");
                rtv!(d.inner(), "inner for dynamic (key)");
                apush!(")");
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => apush!(&self.translate_varname(id.as_str())),

            Pt::IfElse(ie) => {
                apush!("(new ");
                apush!(NIX_LAZY);
                apush!("(function() { let nixRet = undefined; if(");
                apush!(NIX_FORCE);
                apush!("(");
                rtv!(ie.condition(), "condition for if-else");
                apush!(")) { nixRet = ");
                rtv!(ie.body(), "if-body for if-else");
                apush!("; } else { nixRet = ");
                rtv!(ie.else_body(), "else-body for if-else");
                apush!("; }}))");
            }

            Pt::Select(sel) => {
                apush!("((");
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
                    return Err(vec![format!(
                        "{:?}: {} missing",
                        txtrng, "index for selectr"
                    )]);
                }
                apush!("])");
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
                    apush!("((function(){ let nixInhR = ");
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
                    apush!("})())");
                } else {
                    for id in inh.idents() {
                        let idas = id.as_str();
                        apush!(NIX_IN_SCOPE);
                        apush!("(\"");
                        apush!(&escape_str(idas));
                        apush!("\",");
                        apush!(&self.translate_varname(idas));
                        apush!(");");
                    }
                }
            }

            // to be continued...
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
        acc: &mut ret,
        vars: Default::default(),
    }
    .translate_node(parsed.node())?;
    ret += "\n})";
    Ok(ret)
}
