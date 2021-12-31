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
 **/

use rnix::{types::*, StrPart, SyntaxKind as Sk};
use std::borrow::Cow;
use std::collections::HashMap;

type NixNode = SyntaxNode<rnix::NixLanguage>;

const NIX_BUILTINS: &str = "nixBlti";
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
            None => format!("({}[\"{}\"])", NIX_IN_SCOPE, escape_str(s)),
        }
    }

    fn use_or(&mut self, x: Option<NixNode>, alt: &str) -> Result<(), Vec<String>> {
        match x {
            None => {
                *self.acc += alt;
                Ok(())
            },
            Some(x) => self.translate_node(x),
        }
    }

    fn translate_node(&mut self, node: NixNode) -> Result<(), Vec<String>> {
        if node.kind().is_trivia() {
            return Ok(());
        }

        let txtrng = node.text_range();
        let x = match ParsedType::try_from(node.clone()) {
            Err(e) => return Err(vec![format!("{:?}: unable to parse node of kind {:?}", txtrng, e.0)]),
            Ok(x) => x,
        };
        use ParsedType as Pt;

        macro_rules! apush {
            ($x:expr) => {{ *ctx.acc += $x; }}
        }
        macro_rules! rtv {
            ($x:expr, $desc:expr) => {{ match $x {
                None => return Err(vec![format!("{:?}: {} missing", txtrng, $desc)]),
                Some(x) => self.translate_node(x)?,
            }}}
        };

        Ok(match x {
            Pt::Apply(app) => {
                apush!("((");
                rtv!(app.lambda(), "lambda for application");
                apush!(")(");
                rtv!(app.value(), "value for application");
                apush!("))");
            },

            Pt::Assert(art) => {
                apush!("((function() { ");
                apush!(NIX_BUILTINS);
                apush!(".assert(");
                rtv!(art.condition(), "condition for assert");
                apush!("); return (");
                rtv!(art.body(), "body for assert");
                apush!("); })())");
            },

            Pt::Key(key) => {
                let mut fi = true;
                apush!("[");
                for i in d.path() {
                    if fi {
                        fi = false;
                    } else {
                        apush(",");
                    }
                    rtv!(d.inner(), "inner for key");
                }
                apush!("]");
            },

            Pt::Dynamic(d) => {
                // dynamic key component
                apush!(NIX_BUILTINS);
                apush!(".force(");
                rtv!(d.inner(), "inner for dynamic (key)");
                apush!(")");
            },

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => apush!(self.translate_varname(id)),

            Pt::IfElse(ie) => {
                apush!("(new ");
                apush!(NIX_BUILTINS);
                apush!(".Lazy(function() { let nixRet = undefined; if(");
                apush!(NIX_BUILTINS);
                apush!(".force(");
                rtv!(d.condition(), "condition for if-else");
                apush!(")) { nixRet = ");
                rtv!(d.body(), "if-body for if-else");
                apush!("; } else { nixRet = ");
                rtv!(d.else_body(), "else-body for if-else");
                apush!("; }}))");
            },

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
                        self.translate_nodee(idx)?;
                    }
                } else {
                    return Err(vec![format!("{:?}: {} missing", txtrng, "index for selectr")])
                }
                apush!("])");
            },

            Pt::Inherit(inh) => {
                apush!("(");
                apush!(NIX_BUILTINS);
                apush!(".in_scope(nixInScope, undefined, function(nixInScope) {");
                // idk how to handle self-references....
                unimplemented!();
                apush!("}))");
            },

            // to be continued...
        })
    }
}

pub fn translate(s: &str) -> Result<String, Vec<String>> {
    let parsed = rnix::parse(s);

    // return any occured parsing errors
    {
        let errs = parsed.errors().map(|i| i.to_string()).collect();
        if !errs.is_empty() {
            return Err(errs);
        }
    }

    // preamble
    let mut ret = String::new();
    ret += "(function(nixRt) {\n";
    ret += core::include_str!("blti.js");

    ret += &translate_node(parsed)?;

    // postamble
    ret += "})";
    return Ok(ret);
}
