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

mod consts {
    pub const NIX_BUILTINS_RT: &str = "nixBltiRT";
    pub const NIX_OPERATORS: &str = "nixOp";
    pub const NIX_EXTRACT_SCOPE: &str = "nixBlti.extractScope";
    pub const NIX_OR_DEFAULT: &str = "nixBlti.orDefault";
    pub const NIX_RUNTIME: &str = "nixRt";
    pub const NIX_IN_SCOPE: &str = "nixInScope";
    pub const NIX_LAMBDA_ARG_PFX: &str = "nix__";
    pub const NIX_LAMBDA_BOUND: &str = "nixBound";
}
use consts::*;

mod helpers;
use helpers::*;

enum ScopedVar {
    LambdaArg,
}

struct Context<'a> {
    inp: &'a str,
    acc: &'a mut String,
    vars: Vec<(String, ScopedVar)>,
    names: &'a mut Vec<String>,
    mappings: &'a mut Vec<u8>,
    // tracking positions for offset calc
    lp_src: PosTracker,
    lp_dst: PosTracker,
}

enum LetBody {
    Nix(NixNode),
    ExtractScope,
}

type TranslateResult = Result<(), String>;

impl Context<'_> {
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

    fn translate_node_ident(&mut self, sctx: Option<StackCtx>, id: &Ident) -> String {
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
            "abort" | "throw" | "derivation" => {
                // aliased builtins
                self.push(NIX_BUILTINS_RT);
                self.push(".");
                self.push(vn);
            }
            "import" => {
                self.push(NIX_RUNTIME);
                self.push(".");
                self.push(vn);
            }
            "false" | "true" | "null" => self.push(vn),
            _ => {
                let mut inner =
                    |this: &mut Self| match this.vars.iter().rev().find(|(ref i, _)| vn == i) {
                        Some((_, Sv::LambdaArg)) => {
                            this.push(NIX_LAMBDA_ARG_PFX);
                            this.push(&vn.replace("-", "___"));
                        }
                        None => {
                            let startpos = this.acc.len();
                            this.push(NIX_IN_SCOPE);
                            this.push(&if attrelem_raw_safe(vn) {
                                format!(".{}", vn)
                            } else {
                                format!("[{}]", escape_str(vn))
                            });
                            ret = Some(this.acc[startpos..].to_string());
                        }
                    };
                if let Some(sctx) = sctx {
                    self.lazyness_incoming(sctx, LazyTr::Need, move |this, _| inner(this));
                } else {
                    inner(self);
                }
            }
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
            self.translate_node_ident_escape_str(&name);
        } else {
            self.translate_node(mksctx!(WantAwait, false), node.clone())?;
        }
        Ok(())
    }

    fn translate_node_key_element_indexing(&mut self, node: &NixNode) -> TranslateResult {
        if let Some(name) = Ident::cast(node.clone()) {
            self.translate_node_ident_indexing(&name);
        } else {
            self.push("[");
            self.translate_node(mksctx!(WantAwait, false), node.clone())?;
            self.push("]");
        }
        Ok(())
    }

    fn translate_node_kv(
        &mut self,
        value_sctx: StackCtx,
        i: KeyValue,
        scope: &str,
    ) -> TranslateResult {
        let txtrng = i.node().text_range();
        let (kpfi, kpr);
        if let Some(key) = i.key() {
            let mut kpit = key.path();
            kpfi = match kpit.next() {
                Some(kpfi) => kpfi,
                None => {
                    return Err(format!(
                        "line {}: key for key-value pair missing",
                        self.txtrng_to_lineno(txtrng)
                    ))
                }
            };
            kpr = kpit.collect::<Vec<_>>();
        } else {
            return Err(format!(
                "line {}: key for key-value pair missing",
                self.txtrng_to_lineno(txtrng)
            ));
        };

        let value = match i.value() {
            None => {
                return Err(format!(
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
            self.translate_node(value_sctx, value)?;
            self.push(";");
        } else {
            self.push(&format!(
                "if(!Object.prototype.hasOwnProperty.call({},",
                scope
            ));
            self.translate_node_key_element_force_str(&kpfi)?;
            self.push(&format!(")){}", scope)); /* } */
            self.translate_node_key_element_indexing(&kpfi)?;
            self.push("=Object.create(null)");
            self.push(&format!("await {}._deepMerge({}", NIX_OPERATORS, scope));
            // this is a bit cheating because we directly override
            // parts of the attrset instead of round-tripping thru $`scope`.
            self.translate_node_key_element_indexing(&kpfi)?;
            self.push(",");
            self.translate_node(value_sctx, value)?;
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
        value_sctx: StackCtx,
        inh: Inherit,
        scope: &str,
        use_inhtmp: Option<String>,
    ) -> TranslateResult {
        // inherit may be used in self-referential attrsets,
        // and omitting lazy there would be a bad idea.
        // FIXME: how?
        if let Some(inhf) = inh.from() {
            let mut idents: Vec<_> = inh.idents().collect();
            let inhf_sctx = mksctx!(WantAwait, false);
            if idents.len() == 1 {
                let id = idents.remove(0);
                self.push(scope);
                self.translate_node_ident_indexing(&id);
                self.push("=");
                self.lazyness_incoming(value_sctx, LazyTr::Forward, |this, _| {
                    this.rtv(
                        inhf_sctx,
                        inhf.node().text_range(),
                        inhf.inner(),
                        "inner for inherit-from",
                    )?;
                    this.translate_node_ident_indexing(&id);
                    TranslateResult::Ok(())
                })?;
                self.push(";");
            } else {
                let inhf_var = if let Some(x) = &use_inhtmp {
                    self.push("const ");
                    self.push(x);
                    x
                } else {
                    self.push("await (async ()=>{const nixInhR");
                    "nixInhR"
                };
                self.push("=");
                self.lazyness_incoming(inhf_sctx, LazyTr::Need, |this, sctx| {
                    this.rtv(
                        sctx,
                        inhf.node().text_range(),
                        inhf.inner(),
                        "inner for inherit-from",
                    )
                })?;
                self.push(";");
                for id in idents {
                    self.push(scope);
                    self.translate_node_ident_indexing(&id);
                    self.push("=");
                    self.lazyness_incoming(value_sctx, LazyTr::Forward, |this, _| {
                        this.push(inhf_var);
                        this.translate_node_ident_indexing(&id);
                    });
                    self.push(";");
                }
                if use_inhtmp.is_none() {
                    self.push("})()");
                }
                self.push(";");
            }
        } else {
            for id in inh.idents() {
                self.push(scope);
                self.translate_node_ident_indexing(&id);
                self.push("=");
                self.translate_node_ident(Some(value_sctx), &id);
                self.push(";");
            }
        }
        Ok(())
    }

    fn translate_let<EH: EntryHolder>(
        &mut self,
        body_sctx: StackCtx,
        value_sctx: StackCtx,
        node: &EH,
        body: LetBody,
        scope: &str,
    ) -> TranslateResult {
        if node.entries().next().is_none() && node.inherits().next().is_none() {
            // empty attrset
            match body {
                LetBody::Nix(body) => self.translate_node(body_sctx, body)?,
                LetBody::ExtractScope => self.push("Object.create(null)"),
            }
            return Ok(());
        }
        // TODO: is Forward correct here?
        if scope != NIX_IN_SCOPE
            && matches!(body, LetBody::ExtractScope)
            && node.entries().all(|i| {
                i.value().is_some()
                    && i.key().map(|j| {
                        j.path().count() == 1 && Ident::cast(j.path().next().unwrap()).is_some()
                    }) == Some(true)
            })
            && node.inherits().next().is_none()
        {
            self.lazyness_incoming(body_sctx, LazyTr::Forward, |this, _| {
                // optimization: use real object
                this.push("Object.assign(Object.create(null),{");
                let inner = |this: &mut Self, i: KeyValue| {
                    this.translate_node_ident_escape_str(
                        &Ident::cast(i.key().unwrap().path().next().unwrap()).unwrap(),
                    );
                    this.push(":");
                    this.translate_node(mksctx!(Normal, false), i.value().unwrap())?;
                    TranslateResult::Ok(())
                };
                let mut it = node.entries();
                inner(this, it.next().unwrap())?;
                for i in it {
                    this.push(",");
                    inner(this, i)?;
                }
                this.push("})");
                Ok(())
            })
        } else {
            self.lazyness_incoming(body_sctx, LazyTr::Need, |this, _| {
                this.push(&format!("(async {}=>{{", scope));
                for i in node.entries() {
                    this.translate_node_kv(value_sctx, i, scope)?;
                }
                for (n, i) in node.inherits().enumerate() {
                    this.translate_node_inherit(
                        value_sctx,
                        i,
                        scope,
                        Some(format!("nixInhR{}", n)),
                    )?;
                }
                this.push("return ");
                match body {
                    LetBody::Nix(body) => this.translate_node(mksctx!(WantAwait, false), body)?,
                    LetBody::ExtractScope => {
                        this.push(&format!("{}[{}]", scope, NIX_EXTRACT_SCOPE))
                    }
                }
                this.push(";})(nixBlti.mkScope(");
                if scope == NIX_IN_SCOPE {
                    this.push(NIX_IN_SCOPE);
                }
                this.push("))");
                Ok(())
            })
        }
    }

    fn translate_node(&mut self, sctx: StackCtx, node: NixNode) -> TranslateResult {
        if node.kind().is_trivia() {
            return Ok(());
        }

        let txtrng = node.text_range();
        let x = match ParsedType::try_from(node) {
            Err(e) => {
                return Err(format!(
                    "{:?} (line {}): unable to parse node of kind {:?}",
                    txtrng,
                    self.txtrng_to_lineno(txtrng),
                    e.0
                ));
            }
            Ok(x) => x,
        };
        use ParsedType as Pt;

        match x {
            Pt::Apply(app) => {
                self.lazyness_incoming(sctx, LazyTr::Need, |this, _| {
                    this.push("(");
                    this.rtv(
                        mksctx!(WantAwait, false),
                        txtrng,
                        app.lambda(),
                        "lambda for application",
                    )?;
                    this.push(")(");
                    this.rtv(
                        mksctx!(Normal, false),
                        txtrng,
                        app.value(),
                        "value for application",
                    )?;
                    this.push(")");
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Assert(art) => {
                self.lazyness_incoming(sctx, LazyTr::Need, |this, _| {
                    this.push("(async ()=>{await ");
                    this.push(NIX_BUILTINS_RT);
                    this.push(".assert(");
                    let cond = if let Some(cond) = art.condition() {
                        cond
                    } else {
                        return Err(format!(
                            "line {}: condition for assert missing",
                            this.txtrng_to_lineno(txtrng),
                        ));
                    };
                    this.push(&escape_str(&format!(
                        "line {}: {}",
                        this.txtrng_to_lineno(txtrng),
                        cond.text()
                    )));
                    this.push(",");
                    this.translate_node(mksctx!(Normal, false), cond)?;
                    this.push("); return (");
                    this.rtv(
                        mksctx!(WantAwait, false),
                        txtrng,
                        art.body(),
                        "body for assert",
                    )?;
                    this.push("); })()");
                    Ok(())
                })?;
            }

            Pt::AttrSet(ars) => {
                let scope = if ars.recursive() {
                    NIX_IN_SCOPE
                } else {
                    "nixAttrsScope"
                };
                self.translate_let(
                    sctx,
                    mksctx!(Normal, ars.recursive()),
                    &ars,
                    LetBody::ExtractScope,
                    scope,
                )?;
            }

            Pt::BinOp(bo) => {
                let op = if let Some(op) = bo.operator() {
                    op
                } else {
                    return Err(format!(
                        "line {}: operator for binop missing",
                        self.txtrng_to_lineno(txtrng),
                    ));
                };
                use BinOpKind as Bok;
                match op {
                    Bok::IsSet => {
                        self.push("Object.prototype.hasOwnProperty.call(");
                        self.rtv(
                            mksctx!(WantAwait, false),
                            txtrng,
                            bo.lhs(),
                            "lhs for binop ?",
                        )?;
                        self.push(",");
                        if let Some(x) = bo.rhs() {
                            if let Some(y) = Ident::cast(x.clone()) {
                                self.translate_node_ident_escape_str(&y);
                            } else {
                                self.translate_node(mksctx!(WantAwait, false), x)?;
                            }
                        } else {
                            return Err(format!(
                                "line {}: rhs for binop ? missing",
                                self.txtrng_to_lineno(txtrng),
                            ));
                        }
                        self.push(")");
                    }
                    _ => {
                        self.lazyness_incoming(sctx, LazyTr::Need, |this, _| {
                            let mysctx = mksctx!(Normal, false);
                            this.push(&format!("{}.{:?}(", NIX_OPERATORS, op));
                            this.rtv(mysctx, txtrng, bo.lhs(), "lhs for binop")?;
                            this.push(",");
                            this.rtv(mysctx, txtrng, bo.rhs(), "rhs for binop")?;
                            this.push(")");
                            TranslateResult::Ok(())
                        })?;
                    }
                }
            }

            Pt::Dynamic(d) => {
                self.rtv(sctx, txtrng, d.inner(), "inner for dynamic (key)")?;
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => {
                self.translate_node_ident(Some(sctx), &id);
            }

            Pt::IfElse(ie) => {
                self.lazyness_incoming(sctx, LazyTr::Forward, |this, sctx| {
                    this.push("((");
                    this.rtv(
                        mksctx!(WantAwait, false),
                        txtrng,
                        ie.condition(),
                        "condition for if-else",
                    )?;
                    this.push(")?(");
                    this.rtv(sctx, txtrng, ie.body(), "if-body for if-else")?;
                    this.push("):(");
                    this.rtv(sctx, txtrng, ie.else_body(), "else-body for if-else")?;
                    this.push("))");
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Inherit(inh) => self.translate_node_inherit(sctx, inh, NIX_IN_SCOPE, None)?,

            Pt::InheritFrom(inhf) => {
                self.rtv(sctx, txtrng, inhf.inner(), "inner for inherit-from")?
            }

            Pt::Key(key) => unreachable!("standalone key not supported: {:?}", key),
            Pt::KeyValue(kv) => unreachable!("standalone key-value not supported: {:?}", kv),

            Pt::Lambda(lam) => {
                let argx = if let Some(x) = lam.arg() {
                    x
                } else {
                    return Err(format!("lambda ({:?}) with missing argument", lam));
                };
                // FIXME: use guard to truncate vars
                let cur_lamstk = self.vars.len();
                const BODY_SCTX: StackCtx = mksctx!(WantAwait, false);
                self.push("(async ");
                if let Some(y) = Ident::cast(argx.clone()) {
                    let yas = y.as_str();
                    self.vars.push((yas.to_string(), ScopedVar::LambdaArg));
                    self.translate_node_ident(None, &y);
                    self.push("=>(");
                    self.rtv(BODY_SCTX, txtrng, lam.body(), "body for lambda")?;
                    assert!(self.vars.len() >= cur_lamstk);
                    self.vars.truncate(cur_lamstk);
                    self.push(")");
                } else if let Some(y) = Pattern::cast(argx) {
                    let argname = if let Some(z) = y.at() {
                        self.vars
                            .push((z.as_str().to_string(), ScopedVar::LambdaArg));
                        self.translate_node_ident(None, &z)
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
                            self.translate_node_ident(None, &z);
                            self.push(&format!("={}._lambdaA2chk({},", NIX_OPERATORS, argname));
                            self.translate_node_ident_escape_str(&z);
                            if let Some(zdfl) = i.default() {
                                self.push(",");
                                self.translate_node(mksctx!(Normal, true), zdfl)?;
                            }
                            self.push(");");
                        } else {
                            return Err(format!("lambda pattern ({:?}) has entry without name", y));
                        }
                    }
                    // FIXME: handle missing ellipsis

                    self.push("return ");
                    self.rtv(BODY_SCTX, txtrng, lam.body(), "body for lambda")?;
                    assert!(self.vars.len() >= cur_lamstk);
                    self.vars.truncate(cur_lamstk);
                    self.push("}");
                } else {
                    return Err(format!("lambda ({:?}) with invalid argument", lam));
                }
                self.push(")");
            }

            Pt::LegacyLet(l) => self.translate_let(
                sctx,
                mksctx!(Normal, true),
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
                            format!(
                                "line {}: legacy let {{ ... }} without body assignment",
                                self.txtrng_to_lineno(l.node().text_range())
                            )
                        })?,
                ),
                NIX_IN_SCOPE,
            )?,

            Pt::LetIn(l) => self.translate_let(
                sctx,
                mksctx!(Normal, true),
                &l,
                LetBody::Nix(l.body().ok_or_else(|| {
                    format!(
                        "line {}: let ... in ... without body",
                        self.txtrng_to_lineno(l.node().text_range())
                    )
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
                    self.translate_node(mksctx!(Normal, false), i)?;
                }
                self.push("]");
            }

            Pt::OrDefault(od) => {
                self.lazyness_incoming(sctx, LazyTr::Need, |this, _| {
                    this.push(&format!("{}(", NIX_OR_DEFAULT));
                    this.rtv(
                        mksctx!(Normal, true),
                        txtrng,
                        od.index().map(|i| i.node().clone()),
                        "or-default without indexing operation",
                    )?;
                    this.push(",()=>");
                    this.rtv(
                        mksctx!(Normal, true),
                        txtrng,
                        od.default(),
                        "or-default without default",
                    )?;
                    this.push(")");
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Paren(p) => self.rtv(sctx, txtrng, p.inner(), "inner for paren")?,
            Pt::PathWithInterpol(p) => {
                unreachable!("standalone path-with-interpolation not supported: {:?}", p)
            }
            Pt::Pattern(p) => unreachable!("standalone pattern not supported: {:?}", p),
            Pt::PatBind(p) => unreachable!("standalone pattern @ bind not supported: {:?}", p),
            Pt::PatEntry(p) => unreachable!("standalone pattern entry not supported: {:?}", p),

            Pt::Root(r) => self.rtv(sctx, txtrng, r.inner(), "inner for root")?,

            Pt::Select(sel) => {
                self.lazyness_incoming(sctx, LazyTr::Need, |this, _| {
                    this.rtv(
                        mksctx!(WantAwait, false),
                        txtrng,
                        sel.set(),
                        "set for select",
                    )?;
                    if let Some(idx) = sel.index() {
                        this.translate_node_key_element_indexing(&idx)?;
                    } else {
                        return Err(format!("{:?}: {} missing", txtrng, "index for select"));
                    }
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Str(s) => {
                use rnix::value::StrPart as Sp;
                self.lazyness_incoming(sctx, LazyTr::Forward, |this, _| {
                    match s.parts()[..] {
                        [] => this.push("\"\""),
                        [Sp::Literal(ref lit)] => this.push(&escape_str(lit)),
                        ref sxs => {
                            this.push("(");
                            let mut fi = true;
                            for i in sxs.iter().filter(|i| {
                                if let Sp::Literal(lit) = i {
                                    if lit.is_empty() {
                                        return false;
                                    }
                                }
                                true
                            }) {
                                if fi {
                                    fi = false;
                                } else {
                                    this.push("+");
                                }

                                match i {
                                    Sp::Literal(lit) => this.push(&escape_str(lit)),
                                    Sp::Ast(ast) => {
                                        this.push("(");
                                        let txtrng = ast.node().text_range();
                                        this.rtv(
                                            mksctx!(WantAwait, false),
                                            txtrng,
                                            ast.inner(),
                                            "inner for str-interpolate",
                                        )?;
                                        this.push(")");
                                    }
                                }
                            }
                            this.push(")");
                        }
                    }
                    TranslateResult::Ok(())
                })?;
            }

            Pt::StrInterpol(si) => self.rtv(
                mksctx!(WantAwait, false),
                txtrng,
                si.inner(),
                "inner for str-interpolate",
            )?,

            Pt::UnaryOp(uo) => {
                use UnaryOpKind as Uok;
                match uo.operator() {
                    Uok::Invert | Uok::Negate => {}
                }
                self.push(&format!("{}.u_{:?}(", NIX_OPERATORS, uo.operator()));
                self.rtv(
                    mksctx!(Normal, false),
                    txtrng,
                    uo.value(),
                    "value for unary-op",
                )?;
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
                Err(e) => {
                    return Err(format!(
                        "line {}: value deserialization error: {}",
                        self.txtrng_to_lineno(txtrng),
                        e
                    ))
                }
            },

            Pt::With(with) => {
                self.push(&format!("(async {}=>(", NIX_IN_SCOPE));
                self.rtv(
                    mksctx!(WantAwait, false),
                    txtrng,
                    with.body(),
                    "body for 'with' scope",
                )?;
                self.push(&format!("))(nixBlti.mkScopeWith({},", NIX_IN_SCOPE));
                self.rtv(
                    mksctx!(WantAwait, false),
                    txtrng,
                    with.namespace(),
                    "namespace for 'with' scope",
                )?;
                self.push("))");
            }
        }

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
    match (Context {
        inp: s,
        acc: &mut ret,
        vars: Default::default(),
        names: &mut names,
        mappings: &mut mappings,
        lp_src: Default::default(),
        lp_dst: Default::default(),
    }
    .translate_node(mksctx!(Normal, true), parsed.node()))
    {
        Ok(()) => {}
        Err(e) => return Err(vec![e]),
    }
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
