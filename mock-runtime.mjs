class XpError {
    constructor(message, lno) {
        this.message = message;
        this.lno = lno;
    }
}

export function nixRt(lineno) {
    return {
        abort: msg => { throw XpError('FATAL: ' + msg, lineno); },
        throw: msg => { throw XpError(msg, lineno); },
        export: (anchor, path) => {
            console.log('called RT.export with anchor=' + anchor + ' path=' + path);
            return anchor + '://' + path;
        },
        import: (path) => {
            console.log('called RT.import with path=' + path);
            throw Error("no imports supported");
        }
    };
}
