class XpError extends Error {
    constructor(message, options) {
        this.message = message;
        super(message, options);
    }
}

export let nixRt = {
    abort: msg => { throw XpError('FATAL: ' + msg); },
    throw: msg => { throw XpError(msg); },
    export: (anchor, path) => {
        console.log('called RT.export with anchor=' + anchor + ' path=' + path);
        return anchor + '://' + path;
    },
    import: (path) => {
        console.log('called RT.import with path=' + path);
        throw XpError("no imports supported");
    }
}
