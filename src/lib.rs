#![forbid(unused_variables, non_snake_case)]

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

mod consts;
use consts::*;
mod helpers;
use helpers::*;

struct Context<'a> {
    inp: &'a str,
    acc: &'a mut String,
    vars: Vec<(String, IdentCateg)>,
    with_stack: usize,
    names: &'a mut Vec<String>,
    mappings: &'a mut Vec<u8>,
    // tracking positions for offset calc
    line_cache: linetrack::LineCache,
    lp_src: (usize, usize),
    lp_dst: linetrack::PosTrackerExtern,
}

enum LetBody {
    Nix(NixNode),
    ExtractScope,
}

type TranslateResult = Result<(), String>;

impl Context<'_> {
    fn translate_node_ident_escape_str(&mut self, id: &Ident) -> String {
        let ret = escape_str(id.as_str());
        self.snapshot_ident(id.node().text_range(), |this| this.push(&ret));
        ret
    }

    fn translate_node_ident_indexing(&mut self, id: &Ident) -> String {
        let ret = if attrelem_raw_safe(id.as_str()) {
            format!(".{}", id.as_str())
        } else {
            format!("[{}]", escape_str(id.as_str()))
        };
        self.snapshot_ident(id.node().text_range(), |this| this.push(&ret));
        ret
    }

    fn resolve_ident(&self, id: &Ident) -> Result<IdentCateg, String> {
        let vn = id.as_str();
        let tmp = self
            .vars
            .iter()
            .rev()
            .find(|(ref i, _)| vn == i)
            .map(|(_, c)| *c);
        if let Some(ret) = tmp {
            Ok(ret)
        } else if self.with_stack > 0 {
            // no static analysis feasible
            Ok(IdentCateg::WithScopeVar)
        } else {
            Err(format!(
                "line {}: unknown identifier {}",
                self.txtrng_to_lineno(id.node().text_range()),
                vn
            ))
        }
    }

    fn translate_node_ident(
        &mut self,
        sctx: Option<StackCtx>,
        id: &Ident,
    ) -> Result<String, String> {
        let categ = self.resolve_ident(id)?;
        let vn = id.as_str();
        let startpos = self.acc.len();

        // needed to skip the lazy part for attrset access...
        let mut ret = None;
        let mut handle_lazyness = |this: &mut Self, inner: &mut dyn FnMut(&mut Self)| {
            if let Some(sctx) = sctx {
                this.lazyness_incoming(sctx, Tr::Flush, Tr::Flush, Ladj::Back, |this, _| {
                    let startpos = this.acc.len();
                    inner(this);
                    ret = Some(this.acc[startpos..].to_string());
                });
            } else {
                inner(this);
            }
        };

        match categ {
            IdentCateg::Literal(lit) => {
                self.snapshot_ident(id.node().text_range(), |this| this.push(lit))
            }
            IdentCateg::AlBuiltin("builtins") => {
                self.snapshot_ident(id.node().text_range(), |this| {
                    this.push(NIX_BUILTINS_RT);
                })
            }
            IdentCateg::AlBuiltin(ablti) => self.snapshot_ident(id.node().text_range(), |this| {
                this.push(NIX_BUILTINS_RT);
                this.push(".");
                this.push(ablti.strip_prefix("__").unwrap_or(ablti));
            }),
            IdentCateg::LambdaArg => handle_lazyness(self, &mut |this: &mut Self| {
                this.snapshot_ident(id.node().text_range(), |this| {
                    this.push(NIX_LAMBDA_ARG_PFX);
                    this.push(&vn.replace("-", "_$_").replace("'", "_$"));
                })
            }),
            _ => handle_lazyness(self, &mut |this: &mut Self| {
                this.snapshot_ident(id.node().text_range(), |this| {
                    this.push(NIX_IN_SCOPE);
                    this.push(&if attrelem_raw_safe(vn) {
                        format!(".{}", vn)
                    } else {
                        format!("[{}]", escape_str(vn))
                    });
                })
            }),
        }
        Ok(if let Some(x) = ret {
            x
        } else {
            self.acc[startpos..].to_string()
        })
    }

    fn translate_node_key_element_force_str(&mut self, node: &NixNode) -> TranslateResult {
        if let Some(name) = Ident::cast(node.clone()) {
            self.translate_node_ident_escape_str(&name);
        } else {
            self.translate_node(mksctx!(Want, Nothing), node.clone())?;
        }
        Ok(())
    }

    fn translate_node_key_element_indexing(&mut self, node: &NixNode) -> TranslateResult {
        if let Some(name) = Ident::cast(node.clone()) {
            self.translate_node_ident_indexing(&name);
        } else {
            self.push("[");
            self.translate_node(mksctx!(Want, Nothing), node.clone())?;
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
            self.push("=Object.create(null);");
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
            let inhf_sctx = mksctx!(Want, Nothing);
            if idents.len() == 1 {
                let id = idents.remove(0);
                self.push(scope);
                self.translate_node_ident_indexing(&id);
                self.push("=");
                self.lazyness_incoming(
                    value_sctx,
                    Tr::Forward,
                    Tr::Need,
                    Ladj::Front,
                    |this, _| {
                        this.rtv(
                            inhf_sctx,
                            inhf.node().text_range(),
                            inhf.inner(),
                            "inner for inherit-from",
                        )?;
                        this.translate_node_ident_indexing(&id);
                        TranslateResult::Ok(())
                    },
                )?;
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
                self.lazyness_incoming(
                    inhf_sctx,
                    Tr::Need,
                    Tr::Flush,
                    Ladj::Front,
                    |this, sctx| {
                        this.rtv(
                            sctx,
                            inhf.node().text_range(),
                            inhf.inner(),
                            "inner for inherit-from",
                        )
                    },
                )?;
                self.push(";");
                for id in idents {
                    self.push(scope);
                    self.translate_node_ident_indexing(&id);
                    self.push("=");
                    self.lazyness_incoming(
                        value_sctx,
                        Tr::Forward,
                        Tr::Need,
                        Ladj::Front,
                        |this, _| {
                            this.push(inhf_var);
                            this.translate_node_ident_indexing(&id);
                        },
                    );
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
                self.translate_node_ident(Some(value_sctx), &id)?;
                self.push(";");
            }
        }
        Ok(())
    }

    fn translate_let<EH: EntryHolder>(
        &mut self,
        body_sctx: StackCtx,
        values_lazy: bool,
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
        let value_sctx = if values_lazy {
            mksctx!(Nothing, Want)
        } else {
            mksctx!(Nothing, Nothing)
        };
        if scope != NIX_IN_SCOPE
            && matches!(body, LetBody::ExtractScope)
            && node.entries().all(|i| {
                i.value().is_some()
                    && i.key().map(|j| {
                        j.path().count() == 1 && Ident::cast(j.path().next().unwrap()).is_some()
                    }) == Some(true)
            })
            && node
                .inherits()
                .all(|i| i.from().is_none() || i.idents().count() == 1)
        {
            self.lazyness_incoming(
                body_sctx,
                Tr::Forward,
                Tr::Forward,
                Ladj::Front,
                |this, _| {
                    // optimization: use real object
                    this.push("Object.assign(Object.create(null),{");
                    let mut fi = true;
                    let mut handle_fi = move |this: &mut Self| {
                        if fi {
                            fi = false;
                        } else {
                            this.push(",");
                        }
                    };
                    for i in node.entries() {
                        handle_fi(this);
                        this.translate_node_ident_escape_str(
                            &Ident::cast(i.key().unwrap().path().next().unwrap()).unwrap(),
                        );
                        this.push(":");
                        this.translate_node(value_sctx, i.value().unwrap())?;
                    }
                    for (id, inhf) in node
                        .inherits()
                        .flat_map(|inh| inh.idents().map(|i| (i, inh.from())).collect::<Vec<_>>())
                    {
                        handle_fi(this);
                        this.translate_node_ident_escape_str(&id);
                        this.push(":");
                        if let Some(x) = inhf {
                            this.lazyness_incoming(
                                value_sctx,
                                Tr::Flush,
                                Tr::Flush,
                                Ladj::Front,
                                |this, _| {
                                    this.rtv(
                                        mksctx!(Want, Nothing),
                                        x.node().text_range(),
                                        x.inner(),
                                        "inner for inherit-from",
                                    )?;
                                    this.translate_node_ident_indexing(&id);
                                    TranslateResult::Ok(())
                                },
                            )?;
                        } else {
                            this.translate_node_ident(Some(value_sctx), &id)?;
                        }
                    }
                    this.push("})");
                    Ok(())
                },
            )
        } else {
            self.lazyness_incoming(body_sctx, Tr::Need, Tr::Forward, Ladj::Front, |this, _| {
                this.push(&format!("(async {}=>{{", scope));
                let orig_vstkl = this.vars.len();
                if scope == NIX_IN_SCOPE {
                    for i in node
                        .entries()
                        .flat_map(|i| i.key().and_then(|j| j.path().next()))
                        .chain(
                            node.inherits()
                                .flat_map(|i| i.idents())
                                .map(|i| i.node().clone()),
                        )
                    {
                        if let Some(x) = Ident::cast(i) {
                            // register variable names
                            this.vars
                                .push((x.as_str().to_string(), IdentCateg::LetInScopeVar));
                        }
                    }
                }
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
                    LetBody::Nix(body) => this.translate_node(mksctx!(Want, Nothing), body)?,
                    LetBody::ExtractScope => {
                        this.push(&format!("{}[{}]", scope, NIX_EXTRACT_SCOPE))
                    }
                }
                this.push(";})(nixBlti.mkScope(");
                if scope == NIX_IN_SCOPE {
                    this.push(NIX_IN_SCOPE);
                }
                assert!(this.vars.len() >= orig_vstkl);
                this.vars.truncate(orig_vstkl);
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
        self.snapshot_pos(txtrng.start());
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
                self.lazyness_incoming(sctx, Tr::Need, Tr::Need, Ladj::Front, |this, _sctx| {
                    this.push("(");
                    this.rtv(
                        mksctx!(Want, Nothing),
                        txtrng,
                        app.lambda(),
                        "lambda for application",
                    )?;
                    this.push(")(");
                    this.rtv(
                        mksctx!(Nothing, Nothing),
                        txtrng,
                        app.value(),
                        "value for application",
                    )?;
                    this.push(")");
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Assert(art) => {
                self.lazyness_incoming(sctx, Tr::Flush, Tr::Force, Ladj::Front, |this, _| {
                    // NOTE: we rely on the impl.detail of lazyness_incoming
                    // here that no parens are inserted between => and { ... }
                    this.push("{await ");
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
                    this.translate_node(mksctx!(Nothing, Nothing), cond)?;
                    this.push("); return (");
                    this.rtv(
                        mksctx!(Want, Nothing),
                        txtrng,
                        art.body(),
                        "body for assert",
                    )?;
                    this.push("); }");
                    Ok(())
                })?;
            }

            Pt::AttrSet(ars) => {
                let scope = if ars.recursive() {
                    NIX_IN_SCOPE
                } else {
                    "nixAttrsScope"
                };
                self.translate_let(sctx, ars.recursive(), &ars, LetBody::ExtractScope, scope)?;
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
                        self.rtv(mksctx!(Want, Nothing), txtrng, bo.lhs(), "lhs for binop ?")?;
                        self.push(",");
                        if let Some(x) = bo.rhs() {
                            if let Some(y) = Ident::cast(x.clone()) {
                                self.translate_node_ident_escape_str(&y);
                            } else {
                                self.translate_node(mksctx!(Want, Nothing), x)?;
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
                        self.lazyness_incoming(
                            sctx,
                            Tr::Need,
                            Tr::Flush,
                            Ladj::Front,
                            |this, _| {
                                let mysctx = mksctx!(Nothing, Nothing);
                                this.push(&format!("{}.{:?}(", NIX_OPERATORS, op));
                                this.rtv(mysctx, txtrng, bo.lhs(), "lhs for binop")?;
                                this.push(",");
                                this.rtv(mysctx, txtrng, bo.rhs(), "rhs for binop")?;
                                this.push(")");
                                TranslateResult::Ok(())
                            },
                        )?;
                    }
                }
            }

            Pt::Dynamic(d) => {
                self.rtv(sctx, txtrng, d.inner(), "inner for dynamic (key)")?;
            }

            // should be catched by `parsed.errors()...` in `translate(_)`
            Pt::Error(_) => unreachable!(),

            Pt::Ident(id) => {
                self.translate_node_ident(Some(sctx), &id)?;
            }

            Pt::IfElse(ie) => {
                self.lazyness_incoming(sctx, Tr::Flush, Tr::Flush, Ladj::Front, |this, sctx| {
                    this.push("((");
                    this.rtv(
                        mksctx!(Want, Nothing),
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
                const BODY_SCTX: StackCtx = mksctx!(Want, Nothing);
                self.push("(async ");
                if let Some(y) = Ident::cast(argx.clone()) {
                    let yas = y.as_str();
                    self.vars.push((yas.to_string(), IdentCateg::LambdaArg));
                    self.translate_node_ident(None, &y)?;
                    self.push("=>(");
                    self.rtv(BODY_SCTX, txtrng, lam.body(), "body for lambda")?;
                    assert!(self.vars.len() >= cur_lamstk);
                    self.vars.truncate(cur_lamstk);
                    self.push(")");
                } else if let Some(y) = Pattern::cast(argx) {
                    let argname = if let Some(z) = y.at() {
                        self.vars
                            .push((z.as_str().to_string(), IdentCateg::LambdaArg));
                        self.translate_node_ident(None, &z)?
                    } else {
                        self.push(NIX_LAMBDA_BOUND);
                        NIX_LAMBDA_BOUND.to_string()
                    };
                    // register var names
                    let mut entries = Vec::new();
                    for i in y.entries() {
                        if let Some(z) = i.name() {
                            self.vars
                                .push((z.as_str().to_string(), IdentCateg::LambdaArg));
                            entries.push((z, i.default()));
                        } else {
                            return Err(format!("lambda pattern ({:?}) has entry without name", y));
                        }
                    }
                    let entries = entries;
                    self.push("=>{");
                    self.push(&argname);
                    self.push("=await ");
                    self.push(&argname);
                    self.push(";");
                    for (z, dfl) in entries {
                        self.push("let ");
                        self.translate_node_ident(None, &z)?;
                        // NOTE: it should be unnecessary to insert `await` here,
                        // instead, it is inserted at the usage sites.
                        self.push(&format!("={}._lambdaA2chk({},", NIX_OPERATORS, argname));
                        self.translate_node_ident_escape_str(&z);
                        if let Some(zdfl) = dfl {
                            self.push(",");
                            self.translate_node(mksctx!(Nothing, Want), zdfl)?;
                        }
                        self.push(");");
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
                true,
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
                true,
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
                self.lazyness_incoming(sctx, Tr::Forward, Tr::Flush, Ladj::Front, |this, _| {
                    this.push("[");
                    let mut fi = true;
                    for i in l.items() {
                        if fi {
                            fi = false;
                        } else {
                            this.push(",");
                        }
                        this.translate_node(mksctx!(Nothing, Nothing), i)?;
                    }
                    this.push("]");
                    TranslateResult::Ok(())
                })?;
            }

            Pt::OrDefault(od) => {
                self.lazyness_incoming(sctx, Tr::Need, Tr::Need, Ladj::Front, |this, _| {
                    this.push(&format!("{}(", NIX_OR_DEFAULT));
                    this.rtv(
                        mksctx!(Nothing, Want),
                        txtrng,
                        od.index().map(|i| i.node().clone()),
                        "or-default without indexing operation",
                    )?;
                    this.push(",");
                    this.rtv(
                        mksctx!(Nothing, Want),
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
                let idx = if let Some(idx) = sel.index() {
                    idx
                } else {
                    return Err(format!("{:?}: index for select missing", txtrng));
                };

                let (slt, is_wellknown) = if let Some(slt) = sel.set() {
                    if let Some(id) = Ident::cast(slt.clone()) {
                        (
                            slt,
                            matches!(
                                self.resolve_ident(&id),
                                Ok(IdentCateg::Literal(_) | IdentCateg::AlBuiltin(_))
                            ),
                        )
                    } else {
                        (slt, false)
                    }
                } else {
                    return Err(format!("{:?}: set for select missing", txtrng));
                };
                // TODO: improve this mess
                let (xsctx, xtr) = if is_wellknown {
                    (mksctx!(Nothing, Nothing), Tr::Forward)
                } else {
                    (mksctx!(Want, Nothing), Tr::Need)
                };
                self.lazyness_incoming(sctx, xtr, xtr, Ladj::Front, |this, _| {
                    this.translate_node(xsctx, slt)?;
                    this.translate_node_key_element_indexing(&idx)?;
                    TranslateResult::Ok(())
                })?;
            }

            Pt::Str(s) => {
                use rnix::value::StrPart as Sp;
                // NOTE: we do not need to honor lazyness if we just put a
                // literal string here
                match s.parts()[..] {
                    [] => self.push("\"\""),
                    [Sp::Literal(ref lit)] => self.push(&escape_str(lit)),
                    ref sxs => self.lazyness_incoming(
                        sctx,
                        Tr::Forward,
                        Tr::Need,
                        Ladj::Front,
                        |this, _| {
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
                                            mksctx!(Want, Nothing),
                                            txtrng,
                                            ast.inner(),
                                            "inner for str-interpolate",
                                        )?;
                                        this.push(")");
                                    }
                                }
                            }
                            this.push(")");
                            TranslateResult::Ok(())
                        },
                    )?,
                }
            }

            Pt::StrInterpol(si) => self.rtv(
                mksctx!(Want, Nothing),
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
                    mksctx!(Nothing, Nothing),
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
                self.with_stack += 1;
                self.rtv(
                    mksctx!(Want, Nothing),
                    txtrng,
                    with.body(),
                    "body for 'with' scope",
                )?;
                self.with_stack -= 1;
                self.push(&format!("))(nixBlti.mkScopeWith({},", NIX_IN_SCOPE));
                self.rtv(
                    mksctx!(Want, Nothing),
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
        line_cache: linetrack::LineCache::new(s),
        inp: s,
        acc: &mut ret,
        vars: DFL_VARS
            .iter()
            .map(|(name, val)| (name.to_string(), *val))
            .collect(),
        with_stack: 0,
        names: &mut names,
        mappings: &mut mappings,
        lp_src: Default::default(),
        lp_dst: Default::default(),
    }
    .translate_node(mksctx!(Nothing, Want), parsed.node()))
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
