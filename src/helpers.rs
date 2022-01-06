use crate::{Context, TranslateResult};
use rnix::SyntaxNode as NixNode;

pub fn attrelem_raw_safe(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().unwrap().is_ascii_alphabetic()
        && !s.contains(|i: char| !i.is_ascii_alphanumeric())
}

pub fn escape_str(s: &str) -> String {
    serde_json::value::Value::String(s.to_string()).to_string()
}

#[derive(Clone, Copy)]
pub enum LazynessSt {
    DidAwait,
    WantAwait,
    Normal,
}

#[derive(Clone, Copy)]
pub enum LazyTr {
    Need,
    Forward,
}

#[derive(Clone, Copy)]
pub struct StackCtx {
    pub lazyness_st: LazynessSt,
    pub insert_lazy: bool,
}

#[macro_export]
macro_rules! mksctx {
    ($x:ident, $il:ident) => {{
        StackCtx {
            lazyness_st: crate::helpers::LazynessSt::$x,
            insert_lazy: $il,
        }
    }};
}

impl Context<'_> {
    pub(crate) fn push(&mut self, x: &str) {
        *self.acc += x;
    }

    pub(crate) fn lazyness_incoming<R>(
        &mut self,
        mut sctx: StackCtx,
        await_tr: LazyTr,
        inner: impl FnOnce(&mut Self, StackCtx) -> R,
    ) -> R {
        use {LazyTr::*, LazynessSt::*};
        let new_lnst = match (sctx.lazyness_st, await_tr) {
            (DidAwait, _) => Some(DidAwait),
            (WantAwait, Forward) => Some(WantAwait),
            (WantAwait, Need) => None,
            (Normal, Forward) => Some(Normal),
            (Normal, Need) => Some(WantAwait),
        };
        let mut finisher = Vec::new();
        sctx.lazyness_st = if let Some(x) = new_lnst {
            x
        } else {
            self.push("(await ");
            finisher.push(")");
            DidAwait
        };
        if sctx.insert_lazy {
            self.push("(async ()=>(await ");
            finisher.push("))()");
            sctx.lazyness_st = DidAwait;
            sctx.insert_lazy = false;
        }
        let ret = inner(self, sctx);
        for i in finisher.iter().rev() {
            self.push(i);
        }
        ret
    }

    pub(crate) fn snapshot_pos(&mut self, inpos: rnix::TextSize, is_ident: bool) -> Option<()> {
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

    pub(crate) fn txtrng_to_lineno(&self, txtrng: rnix::TextRange) -> usize {
        let bytepos: usize = txtrng.start().into();
        self.inp
            .char_indices()
            .take_while(|(idx, _)| *idx <= bytepos)
            .filter(|(_, c)| *c == '\n')
            .count()
    }

    pub(crate) fn rtv(
        &mut self,
        sctx: StackCtx,
        txtrng: rnix::TextRange,
        x: Option<NixNode>,
        desc: &str,
    ) -> TranslateResult {
        match x {
            None => {
                return Err(format!(
                    "line {}: {} missing",
                    self.txtrng_to_lineno(txtrng),
                    desc
                ));
            }
            Some(x) => self.translate_node(sctx, x),
        }
    }
}
