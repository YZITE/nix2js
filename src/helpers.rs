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

#[derive(Clone, Copy, Debug)]
pub enum St {
    Did,
    Want,
    Nothing,
}

#[derive(Clone, Copy, Debug)]
pub enum Tr {
    Need,
    Forward,
    Flush,
}

#[derive(Clone, Copy, Debug)]
pub struct StackCtx {
    pub await_st: St,
    pub lazy_st: St,
}

#[macro_export]
macro_rules! mksctx {
    ($awaits:ident, $lazys:ident) => {{
        StackCtx {
            await_st: crate::helpers::St::$awaits,
            lazy_st: crate::helpers::St::$lazys,
        }
    }};
}

// merge expectations
fn merge_sttr(st: St, tr: Tr) -> (St, bool) {
    use {St::*, Tr::*};
    let tmp = match tr {
        Forward => Some(st),
        Flush => match st {
            Did | Nothing => Some(st),
            Want => None,
        },
        Need => match st {
            Did => Some(Did),
            Want => None,
            Nothing => Some(Want),
        },
    };
    (tmp.unwrap_or(Did), tmp.is_none())
}

impl Context<'_> {
    pub(crate) fn push(&mut self, x: &str) {
        *self.acc += x;
    }

    pub(crate) fn lazyness_incoming<R>(
        &mut self,
        mut sctx: StackCtx,
        await_tr: Tr,
        lazy_tr: Tr,
        inner: impl FnOnce(&mut Self, StackCtx) -> R,
    ) -> R {
        let (await_st, do_await) = merge_sttr(sctx.await_st, await_tr);
        let (lazy_st, do_lazy) = merge_sttr(sctx.lazy_st, lazy_tr);
        let mut finisher = Vec::new();
        sctx.await_st = await_st;
        sctx.lazy_st = lazy_st;
        if do_await {
            self.push("(await ");
            finisher.push(")");
        }
        if do_lazy {
            self.push("nixBlti.PLazy.from(async ()=>");
            finisher.push(")");
            sctx.await_st = St::Want;
            sctx.lazy_st = St::Nothing;
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
