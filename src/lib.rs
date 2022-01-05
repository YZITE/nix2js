/**
 this crate converts Nix code into javascript code, and generates code
 which is wrapped inside a function which expects a "runtime" object,
 which should contain the following methods:
 - `realise(derivation_attrset)`: should realise a derivation
    (used e.g. for import-from-derivation, also gets linked onto derivations)
 - `export(anchor,path)`: export a path into the nix store
 - `import(to_be_imported_path)`: import a nix file,
    should callback into the parser.

 It also expects a `nixBlti` object as the second argument, which should
 be the objects/namespace of all exported objects of the npm package `nix-builtins`.
**/
// SPDX-License-Identifier: LGPL-2.1-or-later
use rnix::{types::*, SyntaxNode as NixNode};

mod postracker;
use postracker::PosTracker;

const NIX_BUILTINS_RT: &str = "nixBltiRT";
const NIX_OPERATORS: &str = "nixOp";
const NIX_EXTRACT_SCOPE: &str = "nixBlti.extractScope";
const NIX_OR_DEFAULT: &str = "nixBlti.orDefault";
const NIX_RUNTIME: &str = "nixRt";
const NIX_IN_SCOPE: &str = "nixInScope";
const NIX_LAMBDA_ARG_PFX: &str = "nix__";
const NIX_LAMBDA_BOUND: &str = "nixBound";

const NIX_LAZY_START: &str = "(async ()=>(await ";
const NIX_LAZY_HALFEND: &str = ")()";
const NIX_LAZY_END: &str = "))()";

enum ScopedVar {
    LambdaArg,
}

struct Context<'a> {
    inp: &'a str,
    acc: &'a mut String,
    vars: Vec<(String, ScopedVar)>,
    lazyness_stack: Vec<bool>,
    names: &'a mut Vec<String>,
    mappings: &'a mut Vec<u8>,
    // tracking positions for offset calc
    lp_src: PosTracker,
    lp_dst: PosTracker,
}

fn attrelem_raw_safe(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().unwrap().is_ascii_alphabetic()
        && !s.contains(|i: char| !i.is_ascii_alphanumeric())
}

fn escape_str(s: &str) -> String {
    serde_json::value::Value::String(s.to_string()).to_string()
}

macro_rules! err {
    ($x:expr) => {{
        return Err(vec![$x]);
    }};
}

enum LetBody {
    Nix(NixNode),
    ExtractScope,
}

type TranslateResult = Result<(), Vec<String>>;

impl Context<'_> {
    fn push(&mut self, x: &str) {
        *self.acc += x;
    }

    fn snapshot_pos(&mut self, inpos: rnix::TextSize, is_ident: bool) -> Option<()> {
        let (mut lp_src, mut lp_dst) = (self.lp_src, self.lp_dst);
        let (ident, src_oline, src_ocol) =
            lp_src.update(self.inp.as_bytes(), usize::from(inpos))?;
        let (_, dst_oline, dst_ocol) = lp_dst.update(self.acc.as_bytes(), self.acc.len())?;
        let (src_oline, src_ocol): (u32, u32) =
            (src_oline.try_into().unwrap(), src_ocol.try_into().unwrap());
        let (dst_oline, dst_ocol): (u32, u32) =
            (dst_oline.try_into().unwrap(), dst_ocol.try_into().unwrap());

        if dst_oline == 0 && dst_ocol == 0 && src_oline == 0 && src_ocol == 0 {
            return Some(());
        }

        for _ in 0..dst_oline {
            self.mappings.push(b';');
        }
        if dst_oline == 0 && !self.mappings.is_empty() {
            self.mappings.push(b',');
        }
        use vlq::encode as vlqe;
        vlqe(dst_ocol.into(), &mut self.mappings).unwrap();

        if !(src_oline == 0 && src_ocol == 0) {
            vlqe(0, self.mappings).unwrap();
            vlqe(src_oline.into(), &mut self.mappings).unwrap();
            vlqe(src_ocol.into(), &mut self.mappings).unwrap();
            if is_ident {
                if let Ok(ident) = std::str::from_utf8(ident) {
                    // reuse ident if already present
                    let idx = match self.names.iter().enumerate().find(|(_, i)| **i == ident) {
                        Some((idx, _)) => idx,
                        None => {
                            let idx = self.names.len();
                            self.names.push(ident.to_string());
                            idx
                        }
                    };
                    vlqe(idx.try_into().unwrap(), &mut self.mappings).unwrap();
                }
            }
        }

        self.lp_src = lp_src;
        self.lp_dst = lp_dst;
        Some(())
    }

    fn txtrng_to_lineno(&self, txtrng: rnix::TextRange) -> usize {
        let bytepos: usize = txtrng.start().into();
        self.inp
            .char_indices()
            .take_while(|(idx, _)| *idx <= bytepos)
            .filter(|(_, c)| *c == '\n')
            .count()
    }

    fn rtv(&mut self, txtrng: rnix::TextRange, x: Option<NixNode>, desc: &str) -> TranslateResult {
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

    fn translate_node_ident_escape_str(&mut self, id: &Ident) -> String {
        let txtrng = id.node().text_range();
        // if we don't make this conditional, we would record
        // scrambled identifiers otherwise...
        let is_ident = self.snapshot_pos(txtrng.start(), false).is_some();
        let ret = escape_str(id.as_str());
        self.push(&ret);
        self.snapshot_pos(txtrng.end(), is_ident);
        ret
    }

    fn translate_node_ident_indexing(&mut self, id: &Ident) -> String {
        let txtrng = id.node().text_range();
        // if we don't make this conditional, we would record
        // scrambled identifiers otherwise...
        let is_ident = self.snapshot_pos(txtrng.start(), false).is_some();
        let ret = if attrelem_raw_safe(id.as_str()) {
            format!(".{}", id.as_str())
        } else {
            format!("[{}]", escape_str(id.as_str()))
        };
        self.push(&ret);
        self.snapshot_pos(txtrng.end(), is_ident);
        ret
    }

    fn translate_node_ident(&mut self, id: &Ident) -> String {
        let txtrng = id.node().text_range();
        // if we don't make this conditional, we would record
        // scrambled identifiers otherwise...
        let is_ident = self.snapshot_pos(txtrng.start(), false).is_some();
        let vn = id.as_str();

        use ScopedVar as Sv;
        let startpos = self.acc.len();
        // needed to skip the lazy part for attrset access...
        let mut ret = None;
        match vn {
            "builtins" => self.push(NIX_BUILTINS_RT),
            "derivation" => {
                // aliased name for derivation builtin
                self.push(NIX_BUILTINS_RT);
                self.push(".derivation");
            }
            "abort" | "import" | "throw" => {
                self.push(NIX_RUNTIME);
                self.push(".");
                self.push(vn);
            }
            "false" | "true" | "null" => self.push(vn),
            _ => match self.vars.iter().rev().find(|(ref i, _)| vn == i) {
                Some((_, x)) => match x {
                    // TODO: improve this
                    Sv::LambdaArg => {
                        self.push(NIX_LAMBDA_ARG_PFX);
                        self.push(&vn.replace("-", "___"));
                    }
                },
                None => {
                    let insert_async = !*self.lazyness_stack.last().unwrap_or(&false);
                    if insert_async {
                        self.push(NIX_LAZY_START);
                    }
                    let startpos = self.acc.len();
                    self.push(NIX_IN_SCOPE);
                    self.push(&if attrelem_raw_safe(vn) {
                        format!(".{}", vn)
                    } else {
                        format!("[{}]", escape_str(vn))
                    });
                    if insert_async {
                        self.push(NIX_LAZY_END);
                    }
                    ret = Some(self.acc[startpos..].to_string());
                }
            },
        }
        self.snapshot_pos(txtrng.end(), is_ident);
        if let Some(x) = ret {
            x
        } else {
            self.acc[startpos..].to_string()
        }
    }

    fn translate_node_key_element_force_str(&mut self, node: &NixNode) -> TranslateResult {
        if let Some(name) = Ident::cast(node.clone()) {
            let txtrng = name.node().text_range();
            let is_ident = self.snapshot_pos(txtrng.start(), false).is_some();
            self.push(&escape_str(name.as_str()));
            self.snapshot_pos(txtrng.end(), is_ident);
        } else {
            self.translate_node(node.clone())?;
        }
        Ok(())
    }

    fn translate_node_key_element_indexing(&mut self, node: &NixNode) -> TranslateResult {
        if let Some(name) = Ident::cast(node.clone()) {
            self.translate_node_ident_indexing(&name);
        } else {
            self.push("[");
            self.translate_node(node.clone())?;
            self.push("]");
        }
        Ok(())
    }

    fn translate_node_kv(&mut self, i: KeyValue, scope: &str) -> TranslateResult {
        let txtrng = i.node().text_range();
        let (kpfi, kpr);
        if let Some(key) = i.key() {
            let mut kpit = key.path();
            kpfi = match kpit.next() {
                Some(kpfi) => kpfi,
                None => err!(format!(
                    "line {}: key for key-value pair missing",
                    self.txtrng_to_lineno(txtrng)
                )),
            };
            kpr = kpit.collect::<Vec<_>>();
        } else {
            err!(format!(
                "line {}: key for key-value pair missing",
                self.txtrng_to_lineno(txtrng)
            ));
        };

        let value = match i.value() {
            None => {
                err!(format!(
                    "line {}: value for key-value pair missing",
                    self.txtrng_to_lineno(txtrng),
                ));
            }
            Some(x) => x,
        };

        if kpr.is_empty() {
            self.push(scope);
            self.translate_node_key_element_indexing(&kpfi)?;
            self.push("=");
            self.translate_node(value)?;
            self.push(";");
        } else {
            self.push(&format!(
                "if(!Object.prototype.hasOwnProperty.call({},",
                scope
            ));
            self.translate_node_key_element_force_str(&kpfi)?;
            self.push(&format!(")){{{}", scope)); /* } */
            self.translate_node_key_element_indexing(&kpfi)?;
            self.push("=Object.create(null);}");
            self.push(&format!("{}._deepMerge({}", NIX_OPERATORS, scope));
            // this is a bit cheating because we directly override
            // parts of the attrset instead of round-tripping thru $`scope`.
            self.translate_node_key_element_indexing(&kpfi)?;
            self.push(",");
            self.translate_node(value)?;
            for i in kpr {
                self.push(",");
                self.translate_node_key_element_force_str(&i)?;
            }
            self.push(");");
        }
        Ok(())
    }

    fn translate_node_inherit(
        &mut self,
        inh: Inherit,
        scope: &str,
        use_inhtmp: Option<String>,
    ) -> TranslateResult {
        // inherit may be used in self-referential attrsets,
        // and omitting lazy there would be a bad idea.
        self.lazyness_stack.push(true);
        if let Some(inhf) = inh.from() {
            let mut idents: Vec<_> = inh.idents().collect();
            if idents.len() == 1 {
                let id = idents.remove(0);
                self.push(scope);
                self.translate_node_ident_indexing(&id);
                self.push("=");
                self.push(NIX_LAZY_START);
                self.rtv(
                    inhf.node().text_range(),
                    inhf.inner(),
                    "inner for inherit-from",
                )?;
                self.push(")");
                self.translate_node_ident_indexing(&id);
                self.push(NIX_LAZY_HALFEND);
                self.push(";");
            } else {
                if let Some(x) = &use_inhtmp {
                    self.push("const ");
                    self.push(x);
                } else {
                    self.push("await (async ()=>{const nixInhR");
                }
                self.push("=");
                self.rtv(
                    inhf.node().text_range(),
                    inhf.inner(),
                    "inner for inherit-from",
                )?;
                self.push(";");
                for id in idents {
                    self.push(scope);
                    self.translate_node_ident_indexing(&id);
                    self.push("=");
                    self.push(NIX_LAZY_START);
                    self.push("nixInhR)");
                    self.translate_node_ident_indexing(&id);
                    self.push(NIX_LAZY_HALFEND);
                    self.push(";");
                }
                if let Some(x) = &use_inhtmp {
                    self.push("delete ");
                    self.push(x);
                } else {
                    self.push("})()");
                }
                self.push(";");
            }
        } else {
            for id in inh.idents() {
                self.push(scope);
                self.translate_node_ident_indexing(&id);
                self.push("=");
                self.translate_node_ident(&id);
                self.push(";");
            }
        }
        self.lazyness_stack.pop();
        Ok(())
    }

    fn translate_let<EH: EntryHolder>(
        &mut self,
        node: &EH,
        body: LetBody,
        scope: &str,
    ) -> TranslateResult {
        if node.entries().next().is_none() && node.inherits().next().is_none() {
            // empty attrset
            match body {
                LetBody::Nix(body) => self.translate_node(body)?,
                LetBody::ExtractScope => self.push("Object.create(null)"),
            }
            return Ok(());
        }
        self.push(&format!("(async {}=>{{", scope));
        for i in node.entries() {
            self.translate_node_kv(i, scope)?;
        }
        for (n, i) in node.inherits().enumerate() {
            self.translate_node_inherit(i, scope, Some(format!("nixInhR{}", n)))?;
        }
        self.push("return await ");
        match body {
            LetBody::Nix(body) => self.translate_node(body)?,
            LetBody::ExtractScope => self.push(&format!("{}[{}]", scope, NIX_EXTRACT_SCOPE)),
        }
        self.push(";})(nixBlti.mkScope(");
        if scope == NIX_IN_SCOPE {
            self.push(NIX_IN_SCOPE);
        }
        self.push("))");
        Ok(())
    }

    fn translate_node(&mut self, node: NixNode) -> TranslateResult {
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

        let omit_lazy = *self.lazyness_stack.last().unwrap_or(&false);
        self.lazyness_stack.push(false);

        match x {
            Pt::Apply(app) => {
                *self.lazyness_stack.last_mut().unwrap() = true;
                if !omit_lazy {
                    self.push(NIX_LAZY_START);
                }
                self.push("(await (");
                self.rtv(txtrng, app.lambda(), "lambda for application")?;
                self.push("))(");
                self.rtv(txtrng, app.value(), "value for application")?;
                self.push(")");
                if !omit_lazy {
                    self.push(NIX_LAZY_END);
                }
            }

            Pt::Assert(art) => {
                self.push("(async ()=>{await ");
                self.push(NIX_BUILTINS_RT);
                self.push(".assert(");
                let cond = if let Some(cond) = art.condition() {
                    cond
                } else {
                    err!(format!(
                        "line {}: condition for assert missing",
                        self.txtrng_to_lineno(txtrng),
                    ));
                };
                self.push(&escape_str(&format!(
                    "line {}: {}",
                    self.txtrng_to_lineno(txtrng),
                    cond.text()
                )));
                self.push(",");
                self.rtv(txtrng, art.condition(), "condition for assert")?;
                self.push("); return (");
                self.rtv(txtrng, art.body(), "body for assert")?;
                self.push("); })()");
            }

            Pt::AttrSet(ars) => {
                let scope = if ars.recursive() {
                    NIX_IN_SCOPE
                } else {
                    "nixAttrsScope"
                };
                self.translate_let(&ars, LetBody::ExtractScope, scope)?;
            }

            Pt::BinOp(bo) => {
                if let Some(op) = bo.operator() {
                    *self.lazyness_stack.last_mut().unwrap() = true;
                    if !omit_lazy {
                        self.push(NIX_LAZY_START);
                    }
                    use BinOpKind as Bok;
                    match op {
                        Bok::IsSet => {
                            self.push("Object.prototype.hasOwnProperty.call(await ");
                            self.rtv(txtrng, bo.lhs(), "lhs for binop ?")?;
                            self.push(",");
                            if let Some(x) = bo.rhs() {
                                if let Some(y) = Ident::cast(x.clone()) {
                                    self.translate_node_ident_escape_str(&y);
                                } else {
                                    self.push("await ");
                                    self.translate_node(x)?;
                                }
                            } else {
                                err!(format!(
                                    "line {}: rhs for binop ? missing",
                                    self.txtrng_to_lineno(txtrng),
                                ));
                            }
                            self.push(")");
                        }
                        _ => {
                            self.push(&format!("{}.{:?}(", NIX_OPERATORS, op));
                            self.rtv(txtrng, bo.lhs(), "lhs for binop")?;
                            self.push(",");
                            self.rtv(txtrng, bo.rhs(), "rhs for binop")?;
                            self.push(")");
                        }
                    }
                    if !omit_lazy {
                        self.push(NIX_LAZY_END);
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
                *self.lazyness_stack.last_mut().unwrap() = true;
                self.push("(await ");
                self.rtv(txtrng, d.inner(), "inner for dynamic (key)")?;
                self.push(")");
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => {
                self.translate_node_ident(&id);
            }

            Pt::IfElse(ie) => {
                *self.lazyness_stack.last_mut().unwrap() = true;
                if !omit_lazy {
                    self.push(NIX_LAZY_START);
                }
                self.push("((await ");
                self.rtv(txtrng, ie.condition(), "condition for if-else")?;
                self.push(")?(");
                self.rtv(txtrng, ie.body(), "if-body for if-else")?;
                self.push("):(");
                self.rtv(txtrng, ie.else_body(), "else-body for if-else")?;
                self.push("))");
                if !omit_lazy {
                    self.push(NIX_LAZY_END);
                }
            }

            Pt::Inherit(inh) => self.translate_node_inherit(inh, NIX_IN_SCOPE, None)?,

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
                    self.push("(async ");
                    if let Some(y) = Ident::cast(x.clone()) {
                        let yas = y.as_str();
                        self.vars.push((yas.to_string(), ScopedVar::LambdaArg));
                        self.translate_node_ident(&y);
                        self.push("=>(");
                        self.rtv(txtrng, lam.body(), "body for lambda")?;
                        assert!(self.vars.len() >= cur_lamstk);
                        self.vars.truncate(cur_lamstk);
                        self.push(")");
                    } else if let Some(y) = Pattern::cast(x) {
                        let argname = if let Some(z) = y.at() {
                            self.vars
                                .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                            self.translate_node_ident(&z)
                        } else {
                            self.push(NIX_LAMBDA_BOUND);
                            NIX_LAMBDA_BOUND.to_string()
                        };
                        self.push("=>{");
                        self.push(&argname);
                        self.push("=await ");
                        self.push(&argname);
                        self.push(";");
                        for i in y.entries() {
                            if let Some(z) = i.name() {
                                self.push("let ");
                                self.vars
                                    .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                                self.translate_node_ident(&z);
                                self.push("=");
                                if let Some(zdfl) = i.default() {
                                    self.push("(");
                                    let push_argzas = |this: &mut Context<'_>| {
                                        this.push(&argname);
                                        this.translate_node_ident_indexing(&z);
                                    };
                                    push_argzas(self);
                                    self.push(" !==undefined)?(");
                                    push_argzas(self);
                                    self.push("):(");
                                    self.translate_node(zdfl)?;
                                    self.push(")");
                                } else {
                                    self.push(&format!(
                                        "{}._lambdaA2chk({},",
                                        NIX_OPERATORS, argname,
                                    ));
                                    self.translate_node_ident_escape_str(&z);
                                    self.push(")");
                                }
                                self.push(";");
                            } else {
                                err!(format!("lambda pattern ({:?}) has entry without name", y));
                            }
                        }
                        // FIXME: handle missing ellipsis

                        self.push("return ");
                        self.rtv(txtrng, lam.body(), "body for lambda")?;
                        assert!(self.vars.len() >= cur_lamstk);
                        self.vars.truncate(cur_lamstk);
                        self.push("}");
                    } else {
                        err!(format!("lambda ({:?}) with invalid argument", lam));
                    }
                    self.push(")");
                } else {
                    err!(format!("lambda ({:?}) with missing argument", lam));
                }
            }

            Pt::LegacyLet(l) => self.translate_let(
                &l,
                LetBody::Nix(
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
                ),
                NIX_IN_SCOPE,
            )?,

            Pt::LetIn(l) => self.translate_let(
                &l,
                LetBody::Nix(l.body().ok_or_else(|| {
                    vec![format!(
                        "line {}: let ... in ... without body",
                        self.txtrng_to_lineno(l.node().text_range())
                    )]
                })?),
                NIX_IN_SCOPE,
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
                self.push(&format!("{}(async ()=>(", NIX_OR_DEFAULT));
                self.lazyness_stack.push(true);
                self.rtv(
                    txtrng,
                    od.index().map(|i| i.node().clone()),
                    "or-default without indexing operation",
                )?;
                self.lazyness_stack.pop();
                self.push("),");
                self.push(NIX_LAZY_START);
                self.rtv(txtrng, od.default(), "or-default without default")?;
                self.push(NIX_LAZY_END);
                self.push(")");
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
                *self.lazyness_stack.last_mut().unwrap() = true;
                if !omit_lazy {
                    self.push(NIX_LAZY_START);
                }
                self.push("(await ");
                self.rtv(txtrng, sel.set(), "set for select")?;
                self.push(")");
                if let Some(idx) = sel.index() {
                    self.translate_node_key_element_indexing(&idx)?;
                } else {
                    err!(format!("{:?}: {} missing", txtrng, "index for select"));
                }
                if !omit_lazy {
                    self.push(NIX_LAZY_END);
                }
            }

            Pt::Str(s) => {
                *self.lazyness_stack.last_mut().unwrap() = true;
                use rnix::value::StrPart as Sp;
                match s.parts()[..] {
                    [] => self.push("\"\""),
                    [Sp::Literal(ref lit)] => self.push(&escape_str(lit)),
                    ref sxs => {
                        self.push("(");
                        let mut fi = true;
                        for i in sxs {
                            if fi {
                                fi = false;
                            } else {
                                self.push("+");
                            }

                            match i {
                                Sp::Literal(lit) => self.push(&escape_str(lit)),
                                Sp::Ast(ast) => {
                                    self.push("(await ");
                                    let txtrng = ast.node().text_range();
                                    self.rtv(txtrng, ast.inner(), "inner for str-interpolate")?;
                                    self.push(")");
                                }
                            }
                        }
                        self.push(")");
                    }
                }
            }

            Pt::StrInterpol(si) => self.rtv(txtrng, si.inner(), "inner for str-interpolate")?,

            Pt::UnaryOp(uo) => {
                use UnaryOpKind as Uok;
                match uo.operator() {
                    Uok::Invert | Uok::Negate => {}
                }
                self.push(&format!("{}.u_{:?}(", NIX_OPERATORS, uo.operator()));
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
                                "{}.export({},{})",
                                NIX_RUNTIME,
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

            Pt::With(with) => {
                self.push(&format!("(async {}=>(", NIX_IN_SCOPE));
                self.rtv(txtrng, with.body(), "body for 'with' scope")?;
                self.push(&format!("))(nixBlti.mkScopeWith({},", NIX_IN_SCOPE));
                self.rtv(txtrng, with.namespace(), "namespace for 'with' scope")?;
                self.push("))");
            }
        }

        self.lazyness_stack.pop();

        Ok(())
    }
}

pub fn translate(s: &str, inp_name: &str) -> Result<(String, String), Vec<String>> {
    let parsed = rnix::parse(s);

    // return any occured parsing errors
    {
        let errs = parsed.errors();
        if !errs.is_empty() {
            return Err(errs.into_iter().map(|i| i.to_string()).collect());
        }
    }

    let (mut ret, mut names, mut mappings) = (
        String::with_capacity(3 * s.len()),
        Vec::new(),
        Vec::with_capacity((3 * s.len()) / 5),
    );
    ret += "let ";
    ret += NIX_OPERATORS;
    ret += "=nixBlti.nixOp;let ";
    ret += NIX_BUILTINS_RT;
    ret += "=nixBlti.initRtDep(nixRt);let ";
    ret += NIX_IN_SCOPE;
    ret += "=nixBlti.mkScopeWith();return ";
    Context {
        inp: s,
        acc: &mut ret,
        vars: Default::default(),
        lazyness_stack: Vec::new(),
        names: &mut names,
        mappings: &mut mappings,
        lp_src: Default::default(),
        lp_dst: Default::default(),
    }
    .translate_node(parsed.node())?;
    ret += ";";
    let mappings = String::from_utf8(mappings).unwrap();
    Ok((
        ret,
        serde_json::json!({
            "version": 3,
            "sources": [inp_name.to_string()],
            "names": names,
            "mappings": mappings,
        })
        .to_string(),
    ))
}
