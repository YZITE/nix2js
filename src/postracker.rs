/// utility for source-map generation

#[derive(Clone, Copy, Default)]
pub(crate) struct PosTracker {
    offset: usize,
    line: usize,
    column: usize,
}

impl PosTracker {
    // always give `dat` as an argument (but it's start address shouldn't change),
    // to prevent borrowing conflicts or such.
    pub(crate) fn update<'a>(
        &mut self,
        dat: &'a [u8],
        new_offset: usize,
    ) -> Option<(&'a [u8], usize, usize)> {
        new_offset.checked_sub(self.offset)?;
        let mut ldif = 0;
        let mut cdif = 0;
        let slc = &dat[self.offset..new_offset];
        for &i in slc {
            if i == b'\n' {
                cdif = 0;
                ldif += 1;
            } else if i != b'\r' {
                cdif += 1;
            }
        }
        self.offset = new_offset;
        self.line += ldif;
        if ldif != 0 {
            self.column = 0;
        }
        self.column += cdif;
        Some((slc, ldif, cdif))
    }
}
